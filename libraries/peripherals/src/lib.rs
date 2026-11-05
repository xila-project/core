#![no_std]

#[cfg(feature = "adc")]
pub mod adc;

#[cfg(feature = "gpio")]
pub mod gpio;

#[cfg(feature = "i2c")]
pub mod i2c;

#[cfg(feature = "spi")]
pub mod spi;

#[cfg(feature = "uart")]
pub mod uart;
