//! Raw bindings for the startup-supplied native application ABI.
#![no_std]
#![cfg_attr(all(xila_static_pie, not(test)), feature(link_arg_attribute))]

#[cfg(all(feature = "linked", feature = "link"))]
compile_error!(
    "xila-sys: the `linked` and `link` features are mutually exclusive. `linked` builds an \
     application that is linked into the firmware and finds its startup data through the host; \
     `link` builds a standalone static PIE image that the native loader loads. Enable only one \
     of them, and note that Cargo unifies features across a dependency graph."
);

#[rustfmt::skip] // Formatted by the ABI generator.
mod bindings;
pub use bindings::*;

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(feature = "runtime")]
pub mod graphics;

#[cfg(all(xila_static_pie, not(test)))]
include!(concat!(env!("OUT_DIR"), "/link.rs"));
