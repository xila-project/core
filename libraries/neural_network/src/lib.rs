#![no_std]
extern crate alloc;

mod backend;
mod buffer;
mod encoding;
mod loader;

pub use backend::Backend;
pub use buffer::ModelBuffer;
pub use device::neural_network::{LoadRequest, QuantizationEntry, TensorInfo};
pub use encoding::ModelEncoding;
pub use loader::Loader;
