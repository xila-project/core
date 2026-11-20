//! Raw bindings for the startup-supplied native application ABI.
#![no_std]
#![cfg_attr(xila_static_pie, feature(link_arg_attribute))]

#[rustfmt::skip] // Formatted by the ABI generator.
mod bindings;
pub use bindings::*;

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(xila_static_pie)]
include!(concat!(env!("OUT_DIR"), "/link.rs"));
