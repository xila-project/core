#![no_std]

extern crate alloc;

mod null;

pub use null::*;

#[cfg(feature = "wifi_mock")]
pub mod wifi;
