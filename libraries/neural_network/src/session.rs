use nanoglia::{Encoding, Global, Graph, OwnedGraph, Target};
use synchronization::Arc;

use crate::TensorInfo;

// Use Xila's critical-section-backed shared pointer on targets without pointer atomics.
pub(crate) struct SharedGraph<G>(pub Arc<G>);
impl<G> Clone for SharedGraph<G> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
// SAFETY: cloning shares the same immutable graph owner. This wrapper never replaces or
// mutates its owner, and all views borrow the owner for their full lifetime.
unsafe impl<G: nanoglia::GraphHandle<Global>> nanoglia::GraphHandle<Global> for SharedGraph<G> {
    fn graph(&self) -> &Graph<'_, Global> {
        self.0.graph()
    }
}

/// A prepared NN device sharing its immutable graph with each VFS open context.
pub struct NeuralNetworkDevice<G = Graph<'static, Global>> {
    pub(crate) graph: Arc<G>,
}

impl NeuralNetworkDevice<OwnedGraph<Global>> {
    /// Takes ownership of an aligned model buffer, for example filled by a VFS file read.
    pub fn from_owned_model(
        model: nanoglia::core::Block<Global>,
        target: Target,
    ) -> file_system::Result<Self> {
        let graph = OwnedGraph::load_in(model, Encoding::TensorFlowLite, target, Global)
            .map_err(map_error)?;
        Ok(Self {
            graph: Arc::new(graph),
        })
    }
}

impl<'model> NeuralNetworkDevice<Graph<'model, Global>> {
    /// Loads a model from a stable buffer whose base is aligned to `nanoglia::ALIGN`.
    pub fn new(model: &'model [u8], target: Target) -> file_system::Result<Self> {
        if !(model.as_ptr() as usize).is_multiple_of(nanoglia::core::ALIGN) {
            return Err(file_system::Error::InvalidParameter);
        }
        let graph = Graph::load(&[model], Encoding::TensorFlowLite, target).map_err(map_error)?;
        Ok(Self {
            graph: Arc::new(graph),
        })
    }
}

pub(crate) fn tensor_info(desc: nanoglia::TensorDesc<'_>) -> TensorInfo {
    let mut dimensions = [0; nanoglia::core::MAX_RANK];
    let dims = desc.dims.as_slice();
    dimensions[..dims.len()].copy_from_slice(dims);
    TensorInfo {
        data_type: desc.dtype as u32,
        rank: dims.len() as u32,
        dimensions,
        byte_len: desc.byte_len().unwrap_or(0).min(u32::MAX as usize) as u32,
        quantization_axis: if desc.quant.is_quantized() {
            desc.quant.axis
        } else {
            -1
        },
        quantization_count: desc.quant.scale.len().min(u32::MAX as usize) as u32,
    }
}

pub(crate) fn map_error(error: nanoglia::Error) -> file_system::Error {
    match error.code {
        nanoglia::ErrorCode::InvalidArgument => file_system::Error::InvalidParameter,
        nanoglia::ErrorCode::InvalidEncoding => file_system::Error::InvalidFile,
        nanoglia::ErrorCode::TooLarge => file_system::Error::NoMemory,
        nanoglia::ErrorCode::NotFound => file_system::Error::NotFound,
        nanoglia::ErrorCode::UnsupportedOperation => file_system::Error::UnsupportedOperation,
        nanoglia::ErrorCode::Security => file_system::Error::PermissionDenied,
        nanoglia::ErrorCode::Timeout => file_system::Error::RessourceBusy,
        nanoglia::ErrorCode::RuntimeError | nanoglia::ErrorCode::Unknown => {
            file_system::Error::InputOutput
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "cmsis-nn")]
    use alloc::vec;

    #[repr(C, align(16))]
    struct AlignedModel<const N: usize>([u8; N]);

    #[cfg(feature = "cmsis-nn")]
    static MODEL: &AlignedModel<{ include_bytes!("../tests/fixtures/classifier.tflite").len() }> =
        &AlignedModel(*include_bytes!("../tests/fixtures/classifier.tflite"));

    #[test]
    fn rejects_malformed_and_unaligned_models() {
        let malformed = AlignedModel([0u8; 32]);
        assert!(NeuralNetworkDevice::new(&malformed.0, Target::Cpu).is_err());
        let unaligned = AlignedModel([0u8; 33]);
        assert_eq!(
            NeuralNetworkDevice::new(&unaligned.0[1..], Target::Cpu).err(),
            Some(file_system::Error::InvalidParameter)
        );
    }

    #[cfg(feature = "cmsis-nn")]
    #[test]
    fn borrowed_model_graph_can_run_repeatable_inference() {
        let device = NeuralNetworkDevice::new(&MODEL.0, Target::Cpu).unwrap();
        let input = device.graph.input(0).unwrap();
        let mut context = device.graph.init_execution_context().unwrap();
        context
            .set_input(0, &vec![0; input.byte_len().unwrap()])
            .unwrap();
        context.compute().unwrap();
        let expected = context.output(0).unwrap().bytes().to_vec();
        assert!(!expected.is_empty());
        context
            .set_input(0, &vec![0; input.byte_len().unwrap()])
            .unwrap();
        context.compute().unwrap();
        assert_eq!(context.output(0).unwrap().bytes(), expected);
    }
}
