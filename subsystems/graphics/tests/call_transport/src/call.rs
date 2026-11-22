//! The included sources, at the paths they have in `xila-graphics`.

#[path = "../../../src/call/error.rs"]
mod error;
#[path = "../../../src/call/marshal.rs"]
pub mod marshal;
#[allow(
    dead_code,
    reason = "only the accessors the included adapters use are reachable here"
)]
#[path = "../../../src/call/transport.rs"]
pub mod transport;

pub use error::{Error, Result};
pub use transport::{LinearMemory, Transport, Word};
