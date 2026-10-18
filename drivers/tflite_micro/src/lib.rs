//! Private native TensorFlow Lite Micro runtime. The Xila device adapter is separate.
#![no_std]

mod ffi;

mod device;

pub use device::{TfliteMicroContext, TfliteMicroDevice, TfliteMicroModel};

#[cfg(test)]
extern crate std;
