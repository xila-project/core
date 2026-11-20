//! Raw bindings for the startup-supplied native application ABI.
#![no_std]
#![cfg_attr(all(xila_static_pie, not(test)), feature(link_arg_attribute))]

#[rustfmt::skip] // Formatted by the ABI generator.
mod bindings;
pub use bindings::*;

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(all(xila_static_pie, not(test)))]
include!(concat!(env!("OUT_DIR"), "/link.rs"));
