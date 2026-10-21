//! Shared, build-only LVGL guest ABI discovery.

pub mod additional;
mod catalog;
pub mod context;
pub mod enumeration;
pub mod function;
pub mod guest_constants;
pub mod naming;
pub mod registry;

pub use catalog::{Call, CallCatalog, GeneratorError};
pub const MAX_ARGUMENTS: usize = 7;
