#![no_std]
extern crate alloc;
mod command;
mod device;
pub use ::device::neural_network::*;
pub use neural_network::Backend;
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
}
