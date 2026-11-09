#![no_std]

extern crate alloc;

embedded_i18n::i18n!("locales");

mod error;
mod identifiers;
mod manager;

pub use error::*;
pub use identifiers::*;
pub use manager::*;
