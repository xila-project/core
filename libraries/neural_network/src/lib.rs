#![no_std]
mod backend;
pub use backend::Backend;
pub use device::neural_network::{QuantizationEntry, TensorInfo};
