//! Allocation-neutral contract between the VFS adapter and an inference backend.
use crate::{QuantizationEntry, TensorInfo};
use file_system::Result;

/// An immutable prepared model, with independent mutable execution storage per open.
///
/// Implementations own model lifetimes, arena allocation and error translation. Input and
/// output views must refer directly to execution/model storage; the adapter never stages
/// tensors. Backend selection and model loading belong to the runtime assembly.
pub trait Backend: Send + Sync + 'static {
    type Execution: Send + Sync + 'static;
    type Input<'a>: AsMut<[u8]>
    where
        Self: 'a;
    type Output<'a>: AsRef<[u8]>
    where
        Self: 'a;

    fn create_execution(&self) -> Result<Self::Execution>;
    fn owns_execution(&self, execution: &Self::Execution) -> bool;
    fn input_count(&self) -> usize;
    fn output_count(&self) -> usize;
    fn input_info(&self, index: usize) -> Result<TensorInfo>;
    fn output_info(&self, index: usize) -> Result<TensorInfo>;
    fn input_quantization(&self, tensor: usize, entry: usize) -> Result<QuantizationEntry>;
    fn output_quantization(&self, tensor: usize, entry: usize) -> Result<QuantizationEntry>;
    fn input<'a>(
        &self,
        execution: &'a mut Self::Execution,
        index: usize,
    ) -> Result<Self::Input<'a>>;
    fn output<'a>(&self, execution: &'a Self::Execution, index: usize) -> Result<Self::Output<'a>>;
    fn compute(&self, execution: &mut Self::Execution) -> Result<()>;
}
