use alloc::{boxed::Box, vec, vec::Vec};
use core::task::Poll;
use nanoglia::{Global, GraphExecutionContext, GraphHandle};
use synchronization::Arc;

use file_system::{
    BaseOperations, CharacterDevice, Context, ControlCommand, ControlCommandIdentifier, Error,
    MountOperations, Result, Size,
};
use shared::AnyByLayout;

use crate::{
    COMPUTE, GET_INPUT_COUNT, GET_INPUT_INFO, GET_INPUT_QUANTIZATION, GET_OUTPUT_COUNT,
    GET_OUTPUT_INFO, GET_OUTPUT_QUANTIZATION, QuantizationEntry, QuantizationRequest,
    SET_INPUT_INDEX, SET_OUTPUT_INDEX,
    command::validate_command,
    session::{NeuralNetworkDevice, map_error, tensor_info},
};

struct DeviceContext<G: GraphHandle<Allocator = Global>> {
    execution: GraphExecutionContext<super::session::SharedGraph<G>, Global>,
    input_filled: Vec<usize>,
    input_index: usize,
    output_index: usize,
    computed: bool,
}

impl<G: GraphHandle<Allocator = Global> + Send + Sync + 'static> NeuralNetworkDevice<G> {
    fn state<'context>(&self, context: &'context Context) -> Result<&'context DeviceContext<G>> {
        context
            .get_private_data_of_type::<DeviceContext<G>>()
            .filter(|state| Arc::ptr_eq(&state.execution.graph_handle().0, &self.graph))
            .ok_or(Error::InvalidContext)
    }

    fn state_mut<'context>(
        &self,
        context: &'context mut Context,
    ) -> Result<&'context mut DeviceContext<G>> {
        context
            .get_private_data_mutable_of_type::<DeviceContext<G>>()
            .filter(|state| Arc::ptr_eq(&state.execution.graph_handle().0, &self.graph))
            .ok_or(Error::InvalidContext)
    }

    fn compute(&self, state: &mut DeviceContext<G>) -> Result<()> {
        state.computed = false;
        let complete = self
            .graph
            .graph()
            .inputs()
            .zip(&state.input_filled)
            .all(|(desc, filled)| desc.byte_len() == Some(*filled));
        if !complete {
            return Err(Error::InvalidParameter);
        }
        // Arena planning can overwrite inputs even when a kernel returns an error.
        state.input_filled.fill(0);
        state.execution.compute().map_err(map_error)?;
        state.computed = true;
        Ok(())
    }
}

impl<G: GraphHandle<Allocator = Global> + Send + Sync + 'static> BaseOperations
    for NeuralNetworkDevice<G>
{
    fn open(&self, context: &mut Context) -> Result<()> {
        if context.get_private_data().is_some() {
            return Err(Error::InvalidContext);
        }
        let execution =
            GraphExecutionContext::new_in(super::session::SharedGraph(self.graph.clone()), Global)
                .map_err(map_error)?;
        context.set_private_data(Box::new(DeviceContext {
            execution,
            input_filled: vec![0; self.graph.graph().input_count()],
            input_index: 0,
            output_index: 0,
            computed: false,
        }));
        Ok(())
    }

    fn close(&self, context: &mut Context) -> Result<()> {
        self.state(context)?;
        context
            .take_private_data_of_type::<DeviceContext<G>>()
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
            let tensor = state
                .execution
                .output(state.output_index)
                .map_err(map_error)?;
            let output = tensor.bytes();
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
            let mut tensor = state.execution.input_mut(index).map_err(map_error)?;
            let input = tensor.bytes_mut();
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
                    *GET_INPUT_COUNT::cast_output(output)? =
                        self.graph.graph().input_count() as u32;
                }
                GET_INPUT_INFO::IDENTIFIER => {
                    validate_command::<GET_INPUT_INFO>(input, output)?;
                    let index = *GET_INPUT_INFO::cast_input(input)? as usize;
                    *GET_INPUT_INFO::cast_output(output)? =
                        tensor_info(self.graph.graph().input(index).map_err(map_error)?);
                }
                SET_INPUT_INDEX::IDENTIFIER => {
                    validate_command::<SET_INPUT_INDEX>(input, output)?;
                    let index = *SET_INPUT_INDEX::cast_input(input)? as usize;
                    if index >= self.graph.graph().input_count() {
                        return Err(Error::InvalidParameter);
                    }
                    state.input_index = index;
                }
                GET_OUTPUT_COUNT::IDENTIFIER => {
                    validate_command::<GET_OUTPUT_COUNT>(input, output)?;
                    *GET_OUTPUT_COUNT::cast_output(output)? =
                        self.graph.graph().output_count() as u32;
                }
                GET_OUTPUT_INFO::IDENTIFIER => {
                    validate_command::<GET_OUTPUT_INFO>(input, output)?;
                    let index = *GET_OUTPUT_INFO::cast_input(input)? as usize;
                    *GET_OUTPUT_INFO::cast_output(output)? =
                        tensor_info(self.graph.graph().output(index).map_err(map_error)?);
                }
                SET_OUTPUT_INDEX::IDENTIFIER => {
                    validate_command::<SET_OUTPUT_INDEX>(input, output)?;
                    let index = *SET_OUTPUT_INDEX::cast_input(input)? as usize;
                    if index >= self.graph.graph().output_count() {
                        return Err(Error::InvalidParameter);
                    }
                    state.output_index = index;
                }
                GET_INPUT_QUANTIZATION::IDENTIFIER => {
                    validate_command::<GET_INPUT_QUANTIZATION>(input, output)?;
                    let request = *GET_INPUT_QUANTIZATION::cast_input(input)?;
                    let desc = self
                        .graph
                        .graph()
                        .input(request.tensor_index as usize)
                        .map_err(map_error)?;
                    write_quantization(
                        desc.quant,
                        request,
                        GET_INPUT_QUANTIZATION::cast_output(output)?,
                    )?;
                }
                GET_OUTPUT_QUANTIZATION::IDENTIFIER => {
                    validate_command::<GET_OUTPUT_QUANTIZATION>(input, output)?;
                    let request = *GET_OUTPUT_QUANTIZATION::cast_input(input)?;
                    let desc = self
                        .graph
                        .graph()
                        .output(request.tensor_index as usize)
                        .map_err(map_error)?;
                    write_quantization(
                        desc.quant,
                        request,
                        GET_OUTPUT_QUANTIZATION::cast_output(output)?,
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

impl<G: GraphHandle<Allocator = Global> + Send + Sync + 'static> MountOperations
    for NeuralNetworkDevice<G>
{
    fn unmount(&self) -> Result<()> {
        Ok(())
    }
}
impl<G: GraphHandle<Allocator = Global> + Send + Sync + 'static> CharacterDevice
    for NeuralNetworkDevice<G>
{
}

fn write_quantization(
    quantization: nanoglia::Quantization<'_>,
    request: QuantizationRequest,
    output: &mut QuantizationEntry,
) -> Result<()> {
    let index = request.quantization_index as usize;
    output.scale = quantization
        .scale
        .get(index)
        .ok_or(Error::InvalidParameter)?;
    output.zero_point = quantization
        .zero_point
        .get(index)
        .ok_or(Error::InvalidParameter)?;
    Ok(())
}
