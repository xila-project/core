use alloc::{boxed::Box, vec, vec::Vec};
use core::task::Poll;

use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Result, Size,
};
use shared::AnyByLayout;

use crate::{
    Backend, COMPUTE, GET_INPUT_COUNT, GET_INPUT_INFO, GET_INPUT_QUANTIZATION, GET_OUTPUT_COUNT,
    GET_OUTPUT_INFO, GET_OUTPUT_QUANTIZATION, NeuralNetworkDevice, SET_INPUT_INDEX,
    SET_OUTPUT_INDEX, command::validate_command,
};

struct DeviceContext<B: Backend> {
    execution: B::Execution,
    input_filled: Vec<usize>,
    input_index: usize,
    output_index: usize,
    computed: bool,
}

impl<B: Backend> NeuralNetworkDevice<B> {
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

    fn compute(&self, state: &mut DeviceContext<B>) -> Result<()> {
        state.computed = false;
        for (index, filled) in state.input_filled.iter().enumerate() {
            if self.backend.input_info(index)?.byte_len as usize != *filled {
                return Err(Error::InvalidParameter);
            }
        }
        // Arena planning can overwrite inputs even when a kernel returns an error.
        state.input_filled.fill(0);
        self.backend.compute(&mut state.execution)?;
        state.computed = true;
        Ok(())
    }
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
            if !state.computed {
                return Err(Error::NotInitialized);
            }
            let tensor = self.backend.output(&state.execution, state.output_index)?;
            let output = tensor.as_ref();
            let offset = usize::try_from(absolute_position).unwrap_or(usize::MAX);
            if offset >= output.len() {
                return Ok(0);
            }
            let count = buffer.len().min(output.len() - offset);
            buffer[..count].copy_from_slice(&output[offset..offset + count]);
            Ok(count)
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
            let index = state.input_index;
            let filled = *state
                .input_filled
                .get(index)
                .ok_or(Error::InvalidParameter)?;
            let offset = usize::try_from(absolute_position).unwrap_or(usize::MAX);
            if offset > filled {
                return Err(Error::InvalidParameter);
            }
            let mut tensor = self.backend.input(&mut state.execution, index)?;
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
            match command {
                GET_INPUT_COUNT::IDENTIFIER => {
                    validate_command::<GET_INPUT_COUNT>(input, output)?;
                    *GET_INPUT_COUNT::cast_output(output)? = self.backend.input_count() as u32;
                }
                GET_INPUT_INFO::IDENTIFIER => {
                    validate_command::<GET_INPUT_INFO>(input, output)?;
                    let index = *GET_INPUT_INFO::cast_input(input)? as usize;
                    *GET_INPUT_INFO::cast_output(output)? = self.backend.input_info(index)?;
                }
                SET_INPUT_INDEX::IDENTIFIER => {
                    validate_command::<SET_INPUT_INDEX>(input, output)?;
                    let index = *SET_INPUT_INDEX::cast_input(input)? as usize;
                    if index >= self.backend.input_count() {
                        return Err(Error::InvalidParameter);
                    }
                    state.input_index = index;
                }
                GET_OUTPUT_COUNT::IDENTIFIER => {
                    validate_command::<GET_OUTPUT_COUNT>(input, output)?;
                    *GET_OUTPUT_COUNT::cast_output(output)? = self.backend.output_count() as u32;
                }
                GET_OUTPUT_INFO::IDENTIFIER => {
                    validate_command::<GET_OUTPUT_INFO>(input, output)?;
                    let index = *GET_OUTPUT_INFO::cast_input(input)? as usize;
                    *GET_OUTPUT_INFO::cast_output(output)? = self.backend.output_info(index)?;
                }
                SET_OUTPUT_INDEX::IDENTIFIER => {
                    validate_command::<SET_OUTPUT_INDEX>(input, output)?;
                    let index = *SET_OUTPUT_INDEX::cast_input(input)? as usize;
                    if index >= self.backend.output_count() {
                        return Err(Error::InvalidParameter);
                    }
                    state.output_index = index;
                }
                GET_INPUT_QUANTIZATION::IDENTIFIER => {
                    validate_command::<GET_INPUT_QUANTIZATION>(input, output)?;
                    let request = *GET_INPUT_QUANTIZATION::cast_input(input)?;
                    *GET_INPUT_QUANTIZATION::cast_output(output)? =
                        self.backend.input_quantization(
                            request.tensor_index as usize,
                            request.quantization_index as usize,
                        )?;
                }
                GET_OUTPUT_QUANTIZATION::IDENTIFIER => {
                    validate_command::<GET_OUTPUT_QUANTIZATION>(input, output)?;
                    let request = *GET_OUTPUT_QUANTIZATION::cast_input(input)?;
                    *GET_OUTPUT_QUANTIZATION::cast_output(output)? =
                        self.backend.output_quantization(
                            request.tensor_index as usize,
                            request.quantization_index as usize,
                        )?;
                }
                COMPUTE::IDENTIFIER => {
                    validate_command::<COMPUTE>(input, output)?;
                    self.compute(state)?;
                }
                _ => return Err(Error::UnsupportedOperation),
            }
            Ok(())
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
