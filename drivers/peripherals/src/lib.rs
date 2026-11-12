//! Shared base for peripheral device nodes.
//!
//! A class adapter (UART, I2C, SPI, ...) implements [`PeripheralHandle`] over the
//! embedded-hal or embedded-io handle it owns, then wraps it in
//! [`PeripheralDevice`] and mounts it under `/devices/<class>/<name>` with
//! [`mount_peripheral`]. Applications use the ordinary `File` API (open, read,
//! write, poll, close) and receive no new public API from this crate.
//!
//! The base defines no control commands. Each class defines its own commands
//! with `file_system::define_command!` using a distinct kind byte (for example
//! `b'u'` for UART, `b'i'` for I2C, `b's'` for SPI and `b'a'` for ADC). Command
//! input and output structs must be versioned, `#[repr(C)]`, and live in the
//! class module.
#![no_std]

extern crate alloc;

#[cfg(feature = "adc")]
pub mod adc;
mod device;
#[cfg(feature = "gpio")]
pub mod gpio;
#[cfg(feature = "i2c")]
pub mod i2c;
#[cfg(feature = "vfs")]
mod mount;
#[cfg(feature = "power")]
pub mod power;
#[cfg(feature = "pwm")]
pub mod pwm;
#[cfg(feature = "spi")]
pub mod spi;
#[cfg(feature = "uart")]
pub mod uart;

pub use device::*;
#[cfg(feature = "vfs")]
pub use mount::*;
