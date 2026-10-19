#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

mod error;
mod manager;

pub use error::*;
pub use manager::*;

pub type Duration = core::time::Duration;
