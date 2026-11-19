#![no_std]

extern crate alloc;

mod cache;
mod capability;
mod heap;
mod r#macro;
mod manager;
mod protection;
mod region_manager;
mod statistics;
pub mod test;
mod r#trait;
pub mod utilities;

pub use cache::*;
pub use capability::*;
pub use manager::*;
pub use protection::*;
pub use region_manager::*;
pub use statistics::*;
pub use r#trait::*;

pub type Layout = core::alloc::Layout;
