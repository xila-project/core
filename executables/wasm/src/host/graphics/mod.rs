//! VM-independent graphics dispatch. Only `wasmi` depends on the VM API.
mod canvas;
mod dispatch;
pub mod error;
mod events;
mod marshal;
mod objects;
pub mod wasmi;

pub use dispatch::{GraphicsCall, dispatch};
pub use error::GraphicsError;
pub use objects::GraphicsState;

pub(crate) enum ResultKind {
    None,
    Word,
    U32,
    I32,
}
include!(concat!(env!("OUT_DIR"), "/graphics.rs"));
