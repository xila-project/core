/// Serialized or backend-generated model representation accepted by a backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelFormat {
    Tflite,
    Onnx,
    Burn,
    EspDl,
    Other(&'static str),
}

pub struct StaticModel<'a, Aot: ?Sized> {
    pub model: &'a Aot,
    pub format: ModelFormat,
}

impl<Aot: ?Sized> Copy for StaticModel<'_, Aot> {}

impl<Aot: ?Sized> Clone for StaticModel<'_, Aot> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExternalModel<'a> {
    pub location: &'a str,
    pub format: ModelFormat,
}

/// Borrowed model input. `Aot` is the adapter's concrete generated-model type,
/// so static models remain typed and accessible inside the adapter.
pub enum ModelSource<'a, Aot: ?Sized = ()> {
    Buffer { data: &'a [u8], format: ModelFormat },
    Static(StaticModel<'a, Aot>),
    External(ExternalModel<'a>),
}

impl<Aot: ?Sized> Copy for ModelSource<'_, Aot> {}

impl<Aot: ?Sized> Clone for ModelSource<'_, Aot> {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSourceKind {
    Buffer,
    StaticAot,
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TensorType {
    F32,
    F16,
    I8,
    U8,
    I16,
    I32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quantization<'a> {
    PerTensor {
        scale: f32,
        zero_point: i32,
    },
    PerAxis {
        axis: usize,
        scales: &'a [f32],
        zero_points: &'a [i32],
    },
}

#[derive(Clone, Copy, Debug)]
pub struct TensorSpec<'a> {
    shape: &'a [usize],
    data_type: TensorType,
    quantization: Option<Quantization<'a>>,
}

impl<'a> TensorSpec<'a> {
    pub fn try_new(
        shape: &'a [usize],
        data_type: TensorType,
        quantization: Option<Quantization<'a>>,
    ) -> Result<Self, Error> {
        let spec = Self {
            shape,
            data_type,
            quantization,
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn shape(&self) -> &'a [usize] {
        self.shape
    }

    pub fn data_type(&self) -> TensorType {
        self.data_type
    }

    pub fn quantization(&self) -> Option<Quantization<'a>> {
        self.quantization
    }

    pub fn validate(self) -> Result<(), Error> {
        self.element_count()?;
        let valid_parameters = |scale: f32| scale.is_finite() && scale > 0.0;
        let zero_point_range = match self.data_type {
            TensorType::I8 => Some((i8::MIN as i64, i8::MAX as i64)),
            TensorType::U8 => Some((u8::MIN as i64, u8::MAX as i64)),
            TensorType::I16 => Some((i16::MIN as i64, i16::MAX as i64)),
            TensorType::I32 => Some((i32::MIN as i64, i32::MAX as i64)),
            TensorType::F16 | TensorType::F32 => None,
        };
        let valid_zero_point = |zero_point: i32| {
            zero_point_range.is_some_and(|(min, max)| (min..=max).contains(&(zero_point as i64)))
        };
        match self.quantization {
            Some(Quantization::PerTensor { scale, zero_point })
                if !valid_parameters(scale) || !valid_zero_point(zero_point) =>
            {
                Err(Error::InvalidTensor)
            }
            Some(Quantization::PerAxis {
                axis,
                scales,
                zero_points,
            }) if axis >= self.shape.len()
                || self.shape[axis] != scales.len()
                || scales.len() != zero_points.len()
                || !scales.iter().copied().all(valid_parameters) =>
            {
                Err(Error::InvalidTensor)
            }
            Some(Quantization::PerAxis { zero_points, .. })
                if !zero_points.iter().copied().all(valid_zero_point) =>
            {
                Err(Error::InvalidTensor)
            }
            _ => Ok(()),
        }
    }

    pub fn element_count(self) -> Result<usize, Error> {
        self.shape
            .iter()
            .try_fold(1usize, |count, &dimension| count.checked_mul(dimension))
            .ok_or(Error::InvalidTensor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TensorData<'a> {
    F32(&'a [f32]),
    F16(&'a [u16]),
    I8(&'a [i8]),
    U8(&'a [u8]),
    I16(&'a [i16]),
    I32(&'a [i32]),
}

impl TensorData<'_> {
    pub fn data_type(self) -> TensorType {
        match self {
            Self::F32(_) => TensorType::F32,
            Self::F16(_) => TensorType::F16,
            Self::I8(_) => TensorType::I8,
            Self::U8(_) => TensorType::U8,
            Self::I16(_) => TensorType::I16,
            Self::I32(_) => TensorType::I32,
        }
    }

    pub fn len(self) -> usize {
        match self {
            Self::F32(values) => values.len(),
            Self::F16(values) => values.len(),
            Self::I8(values) => values.len(),
            Self::U8(values) => values.len(),
            Self::I16(values) => values.len(),
            Self::I32(values) => values.len(),
        }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TensorView<'a> {
    spec: TensorSpec<'a>,
    data: TensorData<'a>,
}

impl<'a> TensorView<'a> {
    pub fn try_new(spec: TensorSpec<'a>, data: TensorData<'a>) -> Result<Self, Error> {
        spec.validate()?;
        if spec.data_type != data.data_type() || spec.element_count()? != data.len() {
            return Err(Error::InvalidTensor);
        }
        Ok(Self { spec, data })
    }

    pub fn spec(&self) -> TensorSpec<'a> {
        self.spec
    }

    pub fn data(&self) -> TensorData<'a> {
        self.data
    }
}

/// A typed input binding valid for the duration of a synchronous compute call.
#[derive(Clone, Copy, Debug)]
pub struct InputBinding<'a> {
    pub index: usize,
    pub tensor: TensorView<'a>,
}

impl<'a> InputBinding<'a> {
    /// Describes a typed input for one synchronous compute call.
    pub fn new(index: usize, tensor: TensorView<'a>) -> Self {
        Self { index, tensor }
    }
}

/// Caller-owned memory offered to a backend for tensor arena/workspace use.
pub struct Workspace<'a> {
    memory: &'a mut [u8],
}

impl<'a> Workspace<'a> {
    /// Validates caller memory against the backend's reported workspace size
    /// and base-address alignment before making it available to the context.
    pub fn try_new(memory: &'a mut [u8], requirements: MemoryRequirements) -> Result<Self, Error> {
        if !requirements.alignment.is_power_of_two() {
            return Err(Error::InvalidWorkspace);
        }
        if memory.len() < requirements.workspace_bytes {
            return Err(Error::InsufficientMemory);
        }
        if requirements.workspace_bytes > 0
            && !(memory.as_ptr() as usize).is_multiple_of(requirements.alignment)
        {
            return Err(Error::InvalidWorkspace);
        }
        Ok(Self { memory })
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.memory
    }
}

/// Context guard that keeps its model and caller arena borrowed for its lifetime.
///
/// Dropping this value releases those Rust borrows and drops the backend context,
/// but does not call the device's backend-specific teardown. Call
/// [`NeuralNetworkDevice::destroy_context`] exactly once for every successfully
/// created context.
///
/// A context must be dropped before its model can be unloaded:
/// ```compile_fail
/// use device::neural_network::NeuralNetworkDevice;
///
/// fn unload_while_context_is_live<D, A>(device: &mut D, model: D::Model<'static>)
/// where
///     D: NeuralNetworkDevice<A> + 'static,
///     A: ?Sized,
/// {
///     let context = device.create_context(&model, &[], None).unwrap();
///     device.unload_model(model);
///     drop(context);
/// }
/// ```
#[must_use = "call NeuralNetworkDevice::destroy_context before dropping the context"]
pub struct Context<'model, 'workspace, Model, BackendContext> {
    model: &'model Model,
    backend: BackendContext,
    workspace: Option<Workspace<'workspace>>,
}

impl<Model, BackendContext> Context<'_, '_, Model, BackendContext> {
    pub fn model(&self) -> &Model {
        self.model
    }

    pub fn backend(&self) -> &BackendContext {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut BackendContext {
        &mut self.backend
    }

    pub fn workspace(&mut self) -> Option<&mut [u8]> {
        self.workspace.as_mut().map(Workspace::as_mut_slice)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub formats: &'static [ModelFormat],
    pub tensor_types: &'static [TensorType],
    /// Model source variants accepted by this backend; registration and loading
    /// are both performed through `load_model`.
    pub source_modes: &'static [ModelSourceKind],
    pub accelerator: Accelerator,
    pub asynchronous: bool,
    pub zero_copy: bool,
    pub quantization: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Accelerator {
    #[default]
    Cpu,
    Gpu,
    Npu,
    Dsp,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct MemoryRequirements {
    /// Backend-managed model memory estimate.
    pub model_bytes: usize,
    /// Backend-managed context memory estimate.
    pub context_bytes: usize,
    /// Minimum caller-provided workspace size in bytes.
    pub workspace_bytes: usize,
    /// Required power-of-two alignment of the caller-provided workspace base
    /// address. This does not constrain backend-managed allocations.
    pub alignment: usize,
}

impl Default for MemoryRequirements {
    fn default() -> Self {
        Self {
            model_bytes: 0,
            context_bytes: 0,
            workspace_bytes: 0,
            alignment: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    InvalidModel,
    InvalidTensor,
    InsufficientMemory,
    InvalidWorkspace,
    Backend,
}

/// Synchronous, backend-owned neural-network model lifecycle.
///
/// The `Aot` parameter selects a concrete generated model type for static
/// sources. Use `()` when an adapter only consumes byte or external sources.
pub trait NeuralNetworkDevice<Aot: ?Sized = ()> {
    type Model<'model>
    where
        Self: 'model;
    type Context;

    fn capabilities(&self) -> Capabilities;
    fn memory_requirements(
        &self,
        source: ModelSource<'_, Aot>,
    ) -> Result<MemoryRequirements, Error>;
    fn load_model<'model>(
        &mut self,
        source: ModelSource<'model, Aot>,
    ) -> Result<Self::Model<'model>, Error>;
    fn unload_model(&mut self, model: Self::Model<'_>) -> Result<(), Error>;
    fn create_context<'model, 'source: 'model, 'workspace>(
        &mut self,
        model: &'model Self::Model<'source>,
        tensors: &[TensorSpec<'_>],
        mut workspace: Option<Workspace<'workspace>>,
    ) -> Result<Context<'model, 'workspace, Self::Model<'source>, Self::Context>, Error> {
        for tensor in tensors {
            tensor.validate()?;
        }
        let backend = self.initialize_context(
            model,
            tensors,
            workspace.as_mut().map(|arena| arena.as_mut_slice()),
        )?;
        Ok(Context {
            model,
            backend,
            workspace,
        })
    }
    fn initialize_context(
        &mut self,
        model: &Self::Model<'_>,
        tensors: &[TensorSpec<'_>],
        workspace: Option<&mut [u8]>,
    ) -> Result<Self::Context, Error>;
    /// Performs backend-specific teardown and consumes the context. Call this
    /// exactly once for every successful `create_context` call.
    fn destroy_context(
        &mut self,
        context: Context<'_, '_, Self::Model<'_>, Self::Context>,
    ) -> Result<(), Error>;
    fn compute(
        &mut self,
        context: &mut Context<'_, '_, Self::Model<'_>, Self::Context>,
        inputs: &[InputBinding<'_>],
    ) -> Result<(), Error>;
    fn output<'a>(
        &'a self,
        context: &'a Context<'_, '_, Self::Model<'_>, Self::Context>,
        index: usize,
    ) -> Result<TensorView<'a>, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GeneratedModel {
        multiplier: i8,
    }

    struct MockModel<'a> {
        source: ModelSource<'a, GeneratedModel>,
    }
    struct MockContext {
        output: [i8; 3],
        input_address: Option<usize>,
    }
    struct Mock;

    impl NeuralNetworkDevice<GeneratedModel> for Mock {
        type Model<'a> = MockModel<'a>;
        type Context = MockContext;

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                tensor_types: &[TensorType::I8],
                source_modes: &[
                    ModelSourceKind::Buffer,
                    ModelSourceKind::StaticAot,
                    ModelSourceKind::External,
                ],
                ..Capabilities::default()
            }
        }
        fn memory_requirements(
            &self,
            _: ModelSource<'_, GeneratedModel>,
        ) -> Result<MemoryRequirements, Error> {
            Ok(MemoryRequirements::default())
        }
        fn load_model<'a>(
            &mut self,
            source: ModelSource<'a, GeneratedModel>,
        ) -> Result<Self::Model<'a>, Error> {
            if let ModelSource::Static(ref static_model) = source {
                assert_eq!(static_model.model.multiplier, 3);
            }
            Ok(MockModel { source })
        }
        fn unload_model(&mut self, _: Self::Model<'_>) -> Result<(), Error> {
            Ok(())
        }
        fn initialize_context(
            &mut self,
            _: &Self::Model<'_>,
            _: &[TensorSpec<'_>],
            workspace: Option<&mut [u8]>,
        ) -> Result<Self::Context, Error> {
            if let Some(byte) = workspace.and_then(|workspace| workspace.get_mut(1)) {
                *byte = 0x5A;
            }
            Ok(MockContext {
                output: [0; 3],
                input_address: None,
            })
        }
        fn destroy_context(
            &mut self,
            _: Context<'_, '_, Self::Model<'_>, Self::Context>,
        ) -> Result<(), Error> {
            Ok(())
        }
        fn compute(
            &mut self,
            context: &mut Context<'_, '_, Self::Model<'_>, Self::Context>,
            inputs: &[InputBinding<'_>],
        ) -> Result<(), Error> {
            let input = inputs.first().ok_or(Error::InvalidTensor)?;
            match input.tensor.data() {
                TensorData::I8(values) => {
                    context.backend_mut().input_address = Some(values.as_ptr() as usize);
                    context.backend_mut().output.copy_from_slice(values);
                    if let Some(workspace) = context.workspace() {
                        workspace[0] = 0xA5;
                    }
                    Ok(())
                }
                _ => Err(Error::InvalidTensor),
            }
        }
        fn output<'a>(
            &'a self,
            context: &'a Context<'_, '_, Self::Model<'_>, Self::Context>,
            _: usize,
        ) -> Result<TensorView<'a>, Error> {
            TensorView::try_new(
                TensorSpec::try_new(&[3], TensorType::I8, None)?,
                TensorData::I8(&context.backend().output),
            )
        }
    }

    #[test]
    fn all_model_sources_survive_load_context_and_unload_without_copying() {
        let mut device = Mock;
        let model_data = [1_u8, 2, 3];
        let generated = GeneratedModel { multiplier: 3 };
        let sources = [
            ModelSource::Buffer {
                data: &model_data,
                format: ModelFormat::Tflite,
            },
            ModelSource::Static(StaticModel {
                model: &generated,
                format: ModelFormat::Burn,
            }),
            ModelSource::External(ExternalModel {
                location: "flash:/model",
                format: ModelFormat::Tflite,
            }),
        ];
        for source in sources {
            let model = device.load_model(source).unwrap();
            assert!(matches!(
                &model.source,
                ModelSource::Buffer { .. } | ModelSource::Static(_) | ModelSource::External(_)
            ));
            match &model.source {
                ModelSource::Buffer { data, format } => {
                    assert!(core::ptr::eq(data.as_ptr(), model_data.as_ptr()));
                    assert_eq!(*format, ModelFormat::Tflite);
                }
                ModelSource::Static(static_model) => {
                    assert!(core::ptr::eq(static_model.model, &generated));
                    assert_eq!(static_model.format, ModelFormat::Burn);
                }
                ModelSource::External(external) => {
                    assert!(core::ptr::eq(
                        external.location.as_ptr(),
                        "flash:/model".as_ptr()
                    ));
                    assert_eq!(external.format, ModelFormat::Tflite);
                }
            }
            let shape = [1, 3];
            let spec = TensorSpec::try_new(
                &shape,
                TensorType::I8,
                Some(Quantization::PerTensor {
                    scale: 0.25,
                    zero_point: -3,
                }),
            )
            .unwrap();
            let mut arena = [0_u8; 8];
            let requirements = MemoryRequirements {
                workspace_bytes: arena.len(),
                ..MemoryRequirements::default()
            };
            let mut context = device
                .create_context(
                    &model,
                    &[spec],
                    Some(Workspace::try_new(&mut arena, requirements).unwrap()),
                )
                .unwrap();
            assert!(core::ptr::eq(context.model(), &model));
            let input = [5_i8, 6, 7];
            let bindings = [InputBinding::new(
                0,
                TensorView::try_new(spec, TensorData::I8(&input)).unwrap(),
            )];
            assert_eq!(device.capabilities().tensor_types, &[TensorType::I8]);
            assert_eq!(
                device.capabilities().source_modes,
                &[
                    ModelSourceKind::Buffer,
                    ModelSourceKind::StaticAot,
                    ModelSourceKind::External
                ]
            );
            device.compute(&mut context, &bindings).unwrap();
            assert_eq!(
                context.backend().input_address,
                Some(input.as_ptr() as usize)
            );
            assert_eq!(
                device.output(&context, 0).unwrap().data(),
                TensorData::I8(&[5, 6, 7])
            );
            device.destroy_context(context).unwrap();
            assert_eq!(arena[0], 0xA5);
            assert_eq!(arena[1], 0x5A);
            device.unload_model(model).unwrap();
        }
    }

    #[test]
    fn static_sources_preserve_the_concrete_generated_model_type() {
        let generated = GeneratedModel { multiplier: 3 };
        let source: ModelSource<'_, GeneratedModel> = ModelSource::Static(StaticModel {
            model: &generated,
            format: ModelFormat::Burn,
        });
        let ModelSource::Static(model) = source else {
            unreachable!();
        };
        assert_eq!(model.model.multiplier, 3);
    }

    #[test]
    fn tensor_views_reject_type_and_shape_mismatches() {
        let shape = [2];
        let spec = TensorSpec::try_new(&shape, TensorType::I8, None).unwrap();
        assert!(matches!(
            TensorView::try_new(spec, TensorData::U8(&[1, 2])),
            Err(Error::InvalidTensor)
        ));
        assert!(matches!(
            TensorView::try_new(spec, TensorData::I8(&[1])),
            Err(Error::InvalidTensor)
        ));
        assert!(TensorView::try_new(spec, TensorData::I8(&[1, 2])).is_ok());
        assert!(matches!(
            TensorSpec::try_new(&[usize::MAX, 2], TensorType::I8, None),
            Err(Error::InvalidTensor)
        ));
    }

    #[test]
    fn quantization_represents_borrowed_per_axis_parameters() {
        let scales = [0.25, 0.5, 1.0];
        let zero_points = [-2, 0, 3];
        let quantization = Quantization::PerAxis {
            axis: 1,
            scales: &scales,
            zero_points: &zero_points,
        };
        let shape = [1, 3];
        let spec = TensorSpec::try_new(&shape, TensorType::I8, Some(quantization));
        assert!(spec.is_ok());
        assert!(matches!(
            TensorSpec::try_new(
                &shape,
                TensorType::I8,
                Some(Quantization::PerAxis {
                    axis: 1,
                    scales: &scales[..2],
                    zero_points: &zero_points,
                }),
            ),
            Err(Error::InvalidTensor)
        ));
    }

    #[test]
    fn affine_quantization_rejects_float_types_and_out_of_range_zero_points() {
        let shape = [1];
        let quantization = Quantization::PerTensor {
            scale: 1.0,
            zero_point: 0,
        };
        assert!(matches!(
            TensorSpec::try_new(&shape, TensorType::F32, Some(quantization)),
            Err(Error::InvalidTensor)
        ));
        assert!(matches!(
            TensorSpec::try_new(
                &shape,
                TensorType::I8,
                Some(Quantization::PerTensor {
                    scale: 1.0,
                    zero_point: 128,
                }),
            ),
            Err(Error::InvalidTensor)
        ));
        assert!(matches!(
            TensorSpec::try_new(
                &shape,
                TensorType::U8,
                Some(Quantization::PerTensor {
                    scale: 1.0,
                    zero_point: -1,
                }),
            ),
            Err(Error::InvalidTensor)
        ));
        assert!(
            TensorSpec::try_new(
                &shape,
                TensorType::I8,
                Some(Quantization::PerTensor {
                    scale: 1.0,
                    zero_point: -128,
                }),
            )
            .is_ok()
        );
    }

    #[test]
    fn workspace_checks_alignment_and_required_capacity() {
        let requirements = MemoryRequirements {
            workspace_bytes: 8,
            alignment: 8,
            ..MemoryRequirements::default()
        };
        let mut storage = [0_u8; 64];
        let base = storage.as_ptr() as usize;
        let aligned_offset = (8 - base % 8) % 8;
        let aligned = &mut storage[aligned_offset..aligned_offset + 16];
        assert!(Workspace::try_new(aligned, requirements).is_ok());

        let misaligned_offset = aligned_offset + 1;
        assert!(matches!(
            Workspace::try_new(
                &mut storage[misaligned_offset..misaligned_offset + 16],
                requirements
            ),
            Err(Error::InvalidWorkspace)
        ));
        assert!(matches!(
            Workspace::try_new(
                &mut storage[aligned_offset..aligned_offset + 7],
                requirements
            ),
            Err(Error::InsufficientMemory)
        ));
    }

    #[test]
    fn input_binding_is_a_descriptor_passed_to_compute() {
        let shape = [1];
        let spec = TensorSpec::try_new(&shape, TensorType::I8, None).unwrap();
        let values = [7_i8];
        let view = TensorView::try_new(spec, TensorData::I8(&values)).unwrap();
        let binding = InputBinding::new(2, view);
        assert_eq!(binding.index, 2);
        assert_eq!(binding.tensor.data(), TensorData::I8(&values));
    }
}
