//! Dynamic model loader interface.
use crate::{Backend, ModelBuffer, ModelEncoding};
use file_system::Result;

/// Lower-half interface for backends capable of dynamic model loading at runtime.
///
/// An implementation of `Loader` is mounted as a VFS device (e.g. `/devices/nn/cpu`).
/// Guest applications open the node, write the model bytes sequentially, and send
/// the `LOAD` control command to instantiate a [`Backend`].
pub trait Loader: Send + Sync + 'static {
    /// The concrete backend produced by this loader.
    type Backend: Backend;

    /// Parse, verify, and instantiate an inference backend from an owned aligned model buffer.
    fn load(&self, encoding: ModelEncoding, model: ModelBuffer) -> Result<Self::Backend>;

    /// Maximum model size in bytes accepted by this loader.
    ///
    /// Writes beyond this bound return [`file_system::Error::FileTooLarge`].
    fn maximum_model_size(&self) -> usize;
}
