#![no_std]

extern crate alloc;

mod cache;
mod capability;
mod r#macro;
mod manager;
mod protection;
mod statistics;
pub mod test;
mod r#trait;
pub mod utilities;

pub use cache::*;
pub use capability::*;
pub use manager::*;
pub use protection::*;
pub use statistics::*;
pub use r#trait::*;

pub type Layout = core::alloc::Layout;
