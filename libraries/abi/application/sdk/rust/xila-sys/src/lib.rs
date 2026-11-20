//! Raw bindings for the startup-supplied native application ABI.
#![no_std]

#[rustfmt::skip] // Formatted by the ABI generator.
mod bindings;
pub use bindings::*;

#[cfg(feature = "runtime")]
pub mod runtime;
