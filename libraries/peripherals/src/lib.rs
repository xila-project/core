#![no_std]

#[cfg(feature = "adc")]
pub mod adc;

#[cfg(feature = "gpio")]
pub mod gpio;

#[cfg(feature = "i2c")]
pub mod i2c;

#[cfg(feature = "power")]
pub mod power;

#[cfg(feature = "pwm")]
pub mod pwm;

#[cfg(feature = "spi")]
pub mod spi;

#[cfg(feature = "uart")]
pub mod uart;
