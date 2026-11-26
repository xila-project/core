#![no_std]

extern crate alloc;

use littlefs2_sys as littlefs;

mod attributes;
mod callbacks;
mod configuration;
mod constants;
mod directory;
mod error;
mod file;
mod file_system;
mod flags;

use configuration::*;
use directory::*;
use error::*;
use file::*;
pub use file_system::*;
use flags::*;
pub mod stubs;

mod open_metadata;

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    reason = "host-only power-loss tests count device operations with std atomics"
)]
mod power_loss;
