use alloc::{boxed::Box, vec, vec::Vec};
use core::task::Poll;

use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Result, Size,
};
use shared::AnyByLayout;

use crate::{
    Backend, COMPUTE, GET_INPUT_COUNT, GET_INPUT_INFO, GET_INPUT_QUANTIZATION, GET_OUTPUT_COUNT,
    GET_OUTPUT_INFO, GET_OUTPUT_QUANTIZATION, LOAD, LoadRequest, Loader, ModelBuffer,
    ModelEncoding, SET_INPUT_INDEX, SET_OUTPUT_INDEX, command::validate_command,
};

/// A portable VFS character device backed by a statically selected inference backend.
///
/// Contains its backend by value: constructing this adapter adds no allocation or vtable.
pub struct NeuralNetworkDevice<B> {
    pub(crate) backend: B,
}

impl<B: Backend> NeuralNetworkDevice<B> {
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    fn state<'context>(&self, context: &'context Context) -> Result<&'context DeviceContext<B>> {
        context
            .get_private_data_of_type::<DeviceContext<B>>()
            .filter(|state| self.backend.owns_execution(&state.execution))
            .ok_or(Error::InvalidContext)
    }

    fn state_mut<'context>(
        &self,
        context: &'context mut Context,
    ) -> Result<&'context mut DeviceContext<B>> {
        context
            .get_private_data_mutable_of_type::<DeviceContext<B>>()
            .filter(|state| self.backend.owns_execution(&state.execution))
            .ok_or(Error::InvalidContext)
    }
}

/// A portable VFS character device providing runtime model loading.
///
/// A platform mounts an instance of `LoaderDevice` (for example at `/devices/nn/cpu`).
/// Handles start in a staging state where model bytes are written sequentially. After
/// a [`LOAD`] control command is issued, the handle transitions to ready state and
/// behaves identically to a [`NeuralNetworkDevice`] handle.
pub struct LoaderDevice<L> {
    pub(crate) loader: L,
}

impl<L: Loader> LoaderDevice<L> {
    pub const fn new(loader: L) -> Self {
        Self { loader }
    }

    fn state<'context>(
        &self,
        context: &'context Context,
    ) -> Result<&'context LoaderContext<L::Backend>> {
        let loader_id = core::ptr::from_ref(self) as usize;
        context
            .get_private_data_of_type::<LoaderContext<L::Backend>>()
            .filter(|state| match state {
                LoaderContext::Staging { loader_id: id, .. } => *id == loader_id,
                LoaderContext::Ready {
                    backend,
                    device_context,
                    loader_id: id,
                } => *id == loader_id && backend.owns_execution(&device_context.execution),
            })
            .ok_or(Error::InvalidContext)
    }

    fn state_mut<'context>(
        &self,
        context: &'context mut Context,
    ) -> Result<&'context mut LoaderContext<L::Backend>> {
        let loader_id = core::ptr::from_ref(self) as usize;
        context
            .get_private_data_mutable_of_type::<LoaderContext<L::Backend>>()
            .filter(|state| match state {
                LoaderContext::Staging { loader_id: id, .. } => *id == loader_id,
                LoaderContext::Ready {
                    backend,
                    device_context,
                    loader_id: id,
                } => *id == loader_id && backend.owns_execution(&device_context.execution),
            })
            .ok_or(Error::InvalidContext)
    }
}

pub(crate) struct DeviceContext<B: Backend> {
    pub(crate) execution: B::Execution,
    pub(crate) input_filled: Vec<usize>,
    pub(crate) input_index: usize,
    pub(crate) output_index: usize,
    pub(crate) computed: bool,
}

enum LoaderContext<B: Backend> {
    Staging {
        bytes: ModelBuffer,
        loader_id: usize,
    },
    Ready {
        backend: B,
        device_context: DeviceContext<B>,
        loader_id: usize,
    },
}

pub(crate) fn compute<B: Backend>(backend: &B, state: &mut DeviceContext<B>) -> Result<()> {
    state.computed = false;
    for (index, filled) in state.input_filled.iter().enumerate() {
        if backend.input_info(index)?.byte_len as usize != *filled {
            return Err(Error::InvalidParameter);
        }
    }
    // Arena planning can overwrite inputs even when a kernel returns an error.
    state.input_filled.fill(0);
    backend.compute(&mut state.execution)?;
    state.computed = true;
    Ok(())
}

pub(crate) fn handle_inference_read<B: Backend>(
    backend: &B,
    state: &DeviceContext<B>,
    buffer: &mut [u8],
    absolute_position: Size,
) -> Result<usize> {
    if !state.computed {
        return Err(Error::NotInitialized);
    }
    let tensor = backend.output(&state.execution, state.output_index)?;
    let output = tensor.as_ref();
    let offset = usize::try_from(absolute_position).unwrap_or(usize::MAX);
    if offset >= output.len() {
        return Ok(0);
    }
    let count = buffer.len().min(output.len() - offset);
    buffer[..count].copy_from_slice(&output[offset..offset + count]);
    Ok(count)
}

pub(crate) fn handle_inference_write<B: Backend>(
    backend: &B,
    state: &mut DeviceContext<B>,
    buffer: &[u8],
    absolute_position: Size,
) -> Result<usize> {
    let index = state.input_index;
    let filled = *state
        .input_filled
        .get(index)
        .ok_or(Error::InvalidParameter)?;
    let offset = usize::try_from(absolute_position).unwrap_or(usize::MAX);
    if offset > filled {
        return Err(Error::InvalidParameter);
    }
    let mut tensor = backend.input(&mut state.execution, index)?;
    let input = tensor.as_mut();
    if offset >= input.len() {
        return if buffer.is_empty() {
            Ok(0)
        } else {
            Err(Error::FileTooLarge)
        };
    }
    let count = buffer.len().min(input.len() - offset);
    input[offset..offset + count].copy_from_slice(&buffer[..count]);
    state.input_filled[index] = filled.max(offset + count);
    state.computed = false;
    Ok(count)
}

pub(crate) fn handle_inference_control<B: Backend>(
    backend: &B,
    state: &mut DeviceContext<B>,
    command: ControlCommandIdentifier,
    input: &AnyByLayout,
    output: &mut AnyByLayout,
) -> Result<()> {
    match command {
        GET_INPUT_COUNT::IDENTIFIER => {
            validate_command::<GET_INPUT_COUNT>(input, output)?;
            *GET_INPUT_COUNT::cast_output(output)? = backend.input_count() as u32;
        }
        GET_INPUT_INFO::IDENTIFIER => {
            validate_command::<GET_INPUT_INFO>(input, output)?;
            let index = *GET_INPUT_INFO::cast_input(input)? as usize;
            *GET_INPUT_INFO::cast_output(output)? = backend.input_info(index)?;
        }
        SET_INPUT_INDEX::IDENTIFIER => {
            validate_command::<SET_INPUT_INDEX>(input, output)?;
            let index = *SET_INPUT_INDEX::cast_input(input)? as usize;
            if index >= backend.input_count() {
                return Err(Error::InvalidParameter);
            }
            state.input_index = index;
        }
        GET_OUTPUT_COUNT::IDENTIFIER => {
            validate_command::<GET_OUTPUT_COUNT>(input, output)?;
            *GET_OUTPUT_COUNT::cast_output(output)? = backend.output_count() as u32;
        }
        GET_OUTPUT_INFO::IDENTIFIER => {
            validate_command::<GET_OUTPUT_INFO>(input, output)?;
            let index = *GET_OUTPUT_INFO::cast_input(input)? as usize;
            *GET_OUTPUT_INFO::cast_output(output)? = backend.output_info(index)?;
        }
        SET_OUTPUT_INDEX::IDENTIFIER => {
            validate_command::<SET_OUTPUT_INDEX>(input, output)?;
            let index = *SET_OUTPUT_INDEX::cast_input(input)? as usize;
            if index >= backend.output_count() {
                return Err(Error::InvalidParameter);
            }
            state.output_index = index;
        }
        GET_INPUT_QUANTIZATION::IDENTIFIER => {
            validate_command::<GET_INPUT_QUANTIZATION>(input, output)?;
            let request = *GET_INPUT_QUANTIZATION::cast_input(input)?;
            *GET_INPUT_QUANTIZATION::cast_output(output)? = backend.input_quantization(
                request.tensor_index as usize,
                request.quantization_index as usize,
            )?;
        }
        GET_OUTPUT_QUANTIZATION::IDENTIFIER => {
            validate_command::<GET_OUTPUT_QUANTIZATION>(input, output)?;
            let request = *GET_OUTPUT_QUANTIZATION::cast_input(input)?;
            *GET_OUTPUT_QUANTIZATION::cast_output(output)? = backend.output_quantization(
                request.tensor_index as usize,
                request.quantization_index as usize,
            )?;
        }
        COMPUTE::IDENTIFIER => {
            validate_command::<COMPUTE>(input, output)?;
            compute(backend, state)?;
        }
        _ => return Err(Error::UnsupportedOperation),
    }
    Ok(())
}

impl<B: Backend> BaseOperations for NeuralNetworkDevice<B> {
    fn open(&self, context: &mut Context) -> Result<()> {
        if context.get_private_data().is_some() {
            return Err(Error::InvalidContext);
        }
        let execution = self.backend.create_execution()?;
        context.set_private_data(Box::new(DeviceContext::<B> {
            execution,
            input_filled: vec![0; self.backend.input_count()],
            input_index: 0,
            output_index: 0,
            computed: false,
        }));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        self.state(context)?;
        context
            .take_private_data_of_type::<DeviceContext<B>>()
            .map(drop)
            .ok_or(Error::InvalidContext)
    }

    fn read(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        file_system::ready(|| {
            let state = self.state(context)?;
            handle_inference_read(&self.backend, state, buffer, absolute_position)
        })
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        file_system::ready(|| {
            let state = self.state_mut(context)?;
            handle_inference_write(&self.backend, state, buffer, absolute_position)
        })
    }

    fn control(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        file_system::ready(|| {
            let state = self.state_mut(context)?;
            handle_inference_control(&self.backend, state, command, input, output)
        })
    }

    fn clone_context(&self, _context: &Context) -> Result<Context> {
        Err(Error::UnsupportedOperation)
    }
}

impl<B: Backend> MountOperations for NeuralNetworkDevice<B> {
    fn unmount(&self) -> Result<()> {
        Ok(())
    }
}

impl<B: Backend> CharacterDevice for NeuralNetworkDevice<B> {}

impl<L: Loader> BaseOperations for LoaderDevice<L> {
    fn open(&self, context: &mut Context) -> Result<()> {
        if context.get_private_data().is_some() {
            return Err(Error::InvalidContext);
        }
        context.set_private_data(Box::new(LoaderContext::<L::Backend>::Staging {
            bytes: ModelBuffer::new(),
            loader_id: core::ptr::from_ref(self) as usize,
        }));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        self.state(context)?;
        context
            .take_private_data_of_type::<LoaderContext<L::Backend>>()
            .map(drop)
            .ok_or(Error::InvalidContext)
    }

    fn read(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        file_system::ready(|| {
            let state = self.state(context)?;
            match state {
                LoaderContext::Staging { .. } => Err(Error::InvalidContext),
                LoaderContext::Ready {
                    backend,
                    device_context,
                    ..
                } => handle_inference_read(backend, device_context, buffer, absolute_position),
            }
        })
    }

    fn write(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        absolute_position: Size,
    ) -> Poll<Result<usize>> {
        file_system::ready(|| {
            let state = self.state_mut(context)?;
            match state {
                LoaderContext::Staging { bytes, .. } => {
                    let offset = usize::try_from(absolute_position).unwrap_or(usize::MAX);
                    if offset > bytes.len() {
                        return Err(Error::InvalidParameter);
                    }
                    bytes.write_at_within(offset, buffer, self.loader.maximum_model_size())?;
                    Ok(buffer.len())
                }
                LoaderContext::Ready {
                    backend,
                    device_context,
                    ..
                } => handle_inference_write(backend, device_context, buffer, absolute_position),
            }
        })
    }

    fn control(
        &self,
        context: &mut Context,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<Result<()>> {
        file_system::ready(|| {
            let state = self.state_mut(context)?;
            match state {
                LoaderContext::Staging { bytes, loader_id } => {
                    if command != LOAD::IDENTIFIER {
                        return Err(Error::InvalidContext);
                    }
                    validate_command::<LOAD>(input, output)?;
                    let request = *LOAD::cast_input(input)?;
                    if request.version != LoadRequest::VERSION || request.reserved != [0, 0] {
                        return Err(Error::InvalidParameter);
                    }
                    let encoding = ModelEncoding::try_from(request.encoding)?;
                    let model = core::mem::take(bytes);
                    let backend = self.loader.load(encoding, model)?;
                    let execution = backend.create_execution()?;
                    let input_count = backend.input_count();
                    *state = LoaderContext::Ready {
                        backend,
                        device_context: DeviceContext {
                            execution,
                            input_filled: vec![0; input_count],
                            input_index: 0,
                            output_index: 0,
                            computed: false,
                        },
                        loader_id: *loader_id,
                    };
                    Ok(())
                }
                LoaderContext::Ready {
                    backend,
                    device_context,
                    ..
                } => {
                    if command == LOAD::IDENTIFIER {
                        return Err(Error::InvalidContext);
                    }
                    handle_inference_control(backend, device_context, command, input, output)
                }
            }
        })
    }

    fn clone_context(&self, _context: &Context) -> Result<Context> {
        Err(Error::UnsupportedOperation)
    }
}

impl<L: Loader> MountOperations for LoaderDevice<L> {
    fn unmount(&self) -> Result<()> {
        Ok(())
    }
}

impl<L: Loader> CharacterDevice for LoaderDevice<L> {}
