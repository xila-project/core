#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

mod error;
mod manager;
mod realtime;

pub use error::*;
pub use manager::*;
pub use realtime::*;

pub type Duration = core::time::Duration;
