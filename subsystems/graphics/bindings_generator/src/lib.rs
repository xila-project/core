//! Shared, build-only LVGL guest ABI discovery.

pub mod abi;
pub mod additional;
mod catalog;
pub mod context;
pub mod enumeration;
pub mod executor;
pub mod function;
pub mod guest;
pub mod guest_constants;
pub mod naming;
pub mod registry;

pub use catalog::{Call, CallCatalog, GeneratorError};
pub const MAX_ARGUMENTS: usize = 7;

/// The Xila event contract shared by guests and the executor, as Rust source.
/// `xila-graphics` compiles this file as the host's `event_code` module and
/// guest bindings include the same text, so the two cannot drift.
pub const EVENT_CODE_SOURCE: &str = include_str!("event_code.rs");
