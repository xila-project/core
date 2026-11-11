#![no_std]
extern crate alloc;
mod command;
mod device;
pub use ::device::neural_network::*;
pub use device::{LoaderDevice, NeuralNetworkDevice};
pub use neural_network::{Backend, Loader, ModelBuffer, ModelEncoding};

/// Maximum rank in the version-one VFS tensor metadata layout.
pub use ::device::neural_network::MAX_TENSOR_RANK;
