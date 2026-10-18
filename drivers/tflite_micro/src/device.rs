//! Safe Xila adapter for the private TensorFlow Lite Micro C ABI.
use core::{ffi::c_void, marker::PhantomData, ptr::NonNull};

use device::neural_network::{
    Accelerator, Capabilities, Context, Error, InputBinding, MemoryRequirements, ModelFormat,
    ModelSource, ModelSourceKind, NeuralNetworkDevice, Quantization, TensorData, TensorSpec,
    TensorType, TensorView,
};

use crate::ffi;

const MAX_TENSORS: usize = 16;
const MAX_RANK: usize = 6;

const FORMATS: &[ModelFormat] = &[ModelFormat::Tflite];
const SOURCES: &[ModelSourceKind] = &[ModelSourceKind::Buffer];
const TYPES: &[TensorType] = &[TensorType::I8];

/// Stateless CPU backend backed by the pinned, reference-kernel TFLite Micro runtime.
pub struct TfliteMicroDevice;

impl TfliteMicroDevice {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for TfliteMicroDevice {
    fn default() -> Self {
        Self::new()
    }
}

/// A TFLite model kept in the caller's immutable, 8-byte-aligned storage.
pub struct TfliteMicroModel<'model> {
    bytes: &'model [u8],
}

impl TfliteMicroModel<'_> {
    /// The exact borrowed model buffer; the backend never copies model bytes.
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
}

#[derive(Clone, Copy)]
struct TensorMetadata {
    shape: [usize; MAX_RANK],
    rank: usize,
    quantization: Quantization<'static>,
}

impl TensorMetadata {
    const EMPTY: Self = Self {
        shape: [0; MAX_RANK],
        rank: 0,
        quantization: Quantization::PerTensor {
            scale: 1.0,
            zero_point: 0,
        },
    };

    fn matches(&self, spec: TensorSpec<'_>) -> bool {
        spec.data_type() == TensorType::I8
            && spec.shape() == &self.shape[..self.rank]
            && matches!(spec.quantization(), Some(Quantization::PerTensor { scale, zero_point }) if scale == self.scale() && zero_point == self.zero_point())
    }

    fn scale(&self) -> f32 {
        match self.quantization {
            Quantization::PerTensor { scale, .. } => scale,
            Quantization::PerAxis { .. } => unreachable!(),
        }
    }

    fn zero_point(&self) -> i32 {
        match self.quantization {
            Quantization::PerTensor { zero_point, .. } => zero_point,
            Quantization::PerAxis { .. } => unreachable!(),
        }
    }

    fn spec(&self) -> Result<TensorSpec<'_>, Error> {
        TensorSpec::try_new(
            &self.shape[..self.rank],
            TensorType::I8,
            Some(self.quantization),
        )
    }

    fn elements(&self) -> Result<usize, Error> {
        self.shape[..self.rank]
            .iter()
            .try_fold(1usize, |count, &dimension| count.checked_mul(dimension))
            .ok_or(Error::InvalidModel)
    }
}

/// Native interpreter state plus fixed-capacity metadata cached from the model.
///
/// `native` borrows both model bytes and workspace. The enclosing generic
/// [`Context`] stores a borrow of the model and owns the `Workspace`; therefore
/// it cannot outlive either allocation. Calls are exclusive through `&mut
/// Context`, and destruction must happen exactly once via `destroy_context`.
pub struct TfliteMicroContext {
    native: NonNull<c_void>,
    input_count: usize,
    output_count: usize,
    inputs: [TensorMetadata; MAX_TENSORS],
    outputs: [TensorMetadata; MAX_TENSORS],
    _not_send_sync: PhantomData<*mut ()>,
}

fn status_error(status: i32, workspace: bool) -> Error {
    match status {
        ffi::INVALID_MODEL => Error::InvalidModel,
        ffi::UNSUPPORTED => Error::Unsupported,
        ffi::INSUFFICIENT_MEMORY => Error::InsufficientMemory,
        ffi::INVALID_ARGUMENT if workspace => Error::InvalidWorkspace,
        ffi::INVALID_ARGUMENT => Error::InvalidTensor,
        ffi::OUTPUT_UNAVAILABLE => Error::Backend,
        _ => Error::Backend,
    }
}

fn requirements_for(bytes: &[u8]) -> Result<(MemoryRequirements, usize, usize), Error> {
    let mut workspace_bytes = 0;
    let mut input_count = 0;
    let mut output_count = 0;
    let status = unsafe {
        ffi::xila_tflm_model_requirements(
            bytes.as_ptr(),
            bytes.len(),
            &mut workspace_bytes,
            &mut input_count,
            &mut output_count,
        )
    };
    if status != ffi::OK {
        if status == ffi::INVALID_ARGUMENT {
            return Err(Error::InvalidModel);
        }
        return Err(status_error(status, false));
    }
    if input_count == 0
        || output_count == 0
        || input_count > MAX_TENSORS
        || output_count > MAX_TENSORS
    {
        return Err(Error::Unsupported);
    }
    let context_bytes = unsafe { ffi::xila_tflm_context_size() };
    let alignment = unsafe { ffi::xila_tflm_context_alignment() }.max(16);
    if !alignment.is_power_of_two() || workspace_bytes == 0 {
        return Err(Error::Backend);
    }
    Ok((
        MemoryRequirements {
            model_bytes: 0,
            context_bytes,
            workspace_bytes,
            alignment,
        },
        input_count,
        output_count,
    ))
}

fn read_metadata(
    native: NonNull<c_void>,
    input: bool,
    index: usize,
) -> Result<TensorMetadata, Error> {
    let mut kind = 0;
    let mut rank = 0;
    let mut dimensions = [0; MAX_RANK];
    let mut scale = 0.0;
    let mut zero_point = 0;
    let status = unsafe {
        let get_info = if input {
            ffi::xila_tflm_input_info
        } else {
            ffi::xila_tflm_output_info
        };
        get_info(
            native.as_ptr(),
            index,
            &mut kind,
            &mut rank,
            dimensions.as_mut_ptr(),
            dimensions.len(),
            &mut scale,
            &mut zero_point,
        )
    };
    if status != ffi::OK {
        return Err(status_error(status, false));
    }
    if kind != ffi::TYPE_I8 {
        return Err(Error::Unsupported);
    }
    if rank > MAX_RANK || !scale.is_finite() || scale <= 0.0 {
        return Err(Error::Unsupported);
    }
    if dimensions[..rank].contains(&0) || !(i8::MIN as i32..=i8::MAX as i32).contains(&zero_point) {
        return Err(Error::InvalidModel);
    }
    let quantization = Quantization::PerTensor { scale, zero_point };
    Ok(TensorMetadata {
        shape: dimensions,
        rank,
        quantization,
    })
}

impl NeuralNetworkDevice<()> for TfliteMicroDevice {
    type Model<'model> = TfliteMicroModel<'model>;
    type Context = TfliteMicroContext;

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            formats: FORMATS,
            tensor_types: TYPES,
            source_modes: SOURCES,
            accelerator: Accelerator::Cpu,
            asynchronous: false,
            zero_copy: false,
            quantization: true,
        }
    }

    fn memory_requirements(
        &self,
        source: ModelSource<'_, ()>,
    ) -> Result<MemoryRequirements, Error> {
        match source {
            ModelSource::Buffer {
                data,
                format: ModelFormat::Tflite,
            } => requirements_for(data).map(|(requirements, _, _)| requirements),
            _ => Err(Error::Unsupported),
        }
    }

    fn load_model<'model>(
        &mut self,
        source: ModelSource<'model, ()>,
    ) -> Result<Self::Model<'model>, Error> {
        let ModelSource::Buffer {
            data,
            format: ModelFormat::Tflite,
        } = source
        else {
            return Err(Error::Unsupported);
        };
        // The FFI validator checks alignment before dereferencing FlatBuffer data.
        requirements_for(data)?;
        Ok(TfliteMicroModel { bytes: data })
    }

    fn unload_model(&mut self, _model: Self::Model<'_>) -> Result<(), Error> {
        Ok(())
    }

    fn initialize_context(
        &mut self,
        model: &Self::Model<'_>,
        tensors: &[TensorSpec<'_>],
        workspace: Option<&mut [u8]>,
    ) -> Result<Self::Context, Error> {
        let (requirements, input_count, output_count) = requirements_for(model.bytes)?;
        if tensors.len() != input_count {
            return Err(Error::InvalidTensor);
        }
        let workspace = workspace.ok_or(Error::InsufficientMemory)?;
        if workspace.len() < requirements.workspace_bytes {
            return Err(Error::InsufficientMemory);
        }
        if !(workspace.as_ptr() as usize).is_multiple_of(requirements.alignment) {
            return Err(Error::InvalidWorkspace);
        }
        let mut native = core::ptr::null_mut();
        let mut arena_used = 0;
        let status = unsafe {
            ffi::xila_tflm_create(
                model.bytes.as_ptr(),
                model.bytes.len(),
                workspace.as_mut_ptr(),
                workspace.len(),
                &mut native,
                &mut arena_used,
            )
        };
        if status != ffi::OK {
            return Err(status_error(status, true));
        }
        let Some(native) = NonNull::new(native) else {
            return Err(Error::Backend);
        };

        let mut context = TfliteMicroContext {
            native,
            input_count,
            output_count,
            inputs: [TensorMetadata::EMPTY; MAX_TENSORS],
            outputs: [TensorMetadata::EMPTY; MAX_TENSORS],
            _not_send_sync: PhantomData,
        };
        let initialized = (|| {
            for (index, spec) in tensors.iter().enumerate() {
                let metadata = read_metadata(native, true, index)?;
                if !metadata.matches(*spec) {
                    return Err(Error::InvalidTensor);
                }
                context.inputs[index] = metadata;
            }
            for index in 0..output_count {
                context.outputs[index] = read_metadata(native, false, index)?;
            }
            Ok(())
        })();
        if let Err(error) = initialized {
            unsafe { ffi::xila_tflm_destroy(native.as_ptr()) };
            return Err(error);
        }
        let _ = arena_used;
        Ok(context)
    }

    fn destroy_context(
        &mut self,
        context: Context<'_, '_, Self::Model<'_>, Self::Context>,
    ) -> Result<(), Error> {
        unsafe { ffi::xila_tflm_destroy(context.backend().native.as_ptr()) };
        drop(context);
        Ok(())
    }

    fn compute(
        &mut self,
        context: &mut Context<'_, '_, Self::Model<'_>, Self::Context>,
        inputs: &[InputBinding<'_>],
    ) -> Result<(), Error> {
        let backend = context.backend_mut();
        unsafe { ffi::xila_tflm_invalidate_outputs(backend.native.as_ptr()) };
        if inputs.len() != backend.input_count {
            return Err(Error::InvalidTensor);
        }
        for (position, binding) in inputs.iter().enumerate() {
            if binding.index >= backend.input_count
                || inputs[..position]
                    .iter()
                    .any(|previous| previous.index == binding.index)
            {
                return Err(Error::InvalidTensor);
            }
            let metadata = &backend.inputs[binding.index];
            if !metadata.matches(binding.tensor.spec()) {
                return Err(Error::InvalidTensor);
            }
            match binding.tensor.data() {
                TensorData::I8(data) if data.len() == metadata.elements()? => {}
                _ => return Err(Error::InvalidTensor),
            }
        }
        for binding in inputs {
            let TensorData::I8(data) = binding.tensor.data() else {
                return Err(Error::InvalidTensor);
            };
            let status = unsafe {
                ffi::xila_tflm_set_input(
                    backend.native.as_ptr(),
                    binding.index,
                    ffi::TYPE_I8,
                    data.as_ptr().cast(),
                    data.len(),
                )
            };
            if status != ffi::OK {
                return Err(status_error(status, false));
            }
        }
        let status = unsafe { ffi::xila_tflm_invoke(backend.native.as_ptr()) };
        if status != ffi::OK {
            return Err(status_error(status, false));
        }
        Ok(())
    }

    fn output<'a>(
        &'a self,
        context: &'a Context<'_, '_, Self::Model<'_>, Self::Context>,
        index: usize,
    ) -> Result<TensorView<'a>, Error> {
        let backend = context.backend();
        if index >= backend.output_count {
            return Err(Error::InvalidTensor);
        }
        let metadata = &backend.outputs[index];
        let mut kind = 0;
        let mut data = core::ptr::null();
        let mut bytes = 0;
        let status = unsafe {
            ffi::xila_tflm_output_data(
                backend.native.as_ptr(),
                index,
                &mut kind,
                &mut data,
                &mut bytes,
            )
        };
        if status != ffi::OK {
            return Err(status_error(status, false));
        }
        if kind != ffi::TYPE_I8 || bytes != metadata.elements()? || data.is_null() {
            return Err(Error::Backend);
        }
        let spec = metadata.spec()?;
        // The native pointer is valid while this context immutably borrows its
        // exclusive workspace. `output` borrows that context for the view's life.
        let values = unsafe { core::slice::from_raw_parts(data.cast::<i8>(), bytes) };
        TensorView::try_new(spec, TensorData::I8(values))
    }
}
