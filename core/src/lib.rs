//! Xila Core façade crate.
//!
//! This crate re-exports the core subsystems, libraries, and drivers that form
//! the Xila host runtime:
//!
//! - **Subsystems**: [`task`], [`lifecycle`], [`virtual_file_system`], [`users`],
//!   [`time`], [`log`], [`memory`], and (with the `network` and `graphics` features)
//!   [`network`] and [`graphics`].
//! - **System libraries & storage**: [`authentication`], [`executable`],
//!   [`file_system`], [`little_fs`], [`memory_file_system`], [`internationalization`],
//!   [`shared`], and [`synchronization`].
//! - **Optional capabilities**: [`audio`] (`audio`), [`neural_network`] and
//!   [`drivers_neural_network`] (`neural_network`), and [`bootsplash`] (`bootsplash`).

#![no_std]

pub mod about;

#[cfg(feature = "audio")]
pub use audio;

#[cfg(feature = "neural_network")]
pub use drivers_neural_network;
#[cfg(feature = "neural_network")]
pub use neural_network;

pub use authentication;
pub use embedded_i18n as internationalization;
pub use executable;
pub use file_system;
pub use file_system::io_error;
pub use lifecycle;
pub use little_fs;
pub use log;
pub use memory;
pub use memory_file_system;
#[cfg(feature = "network")]
pub use network;
pub use shared;
pub use synchronization;
pub use task;
pub use time;
pub use users;
pub use virtual_file_system;

#[cfg(feature = "bootsplash")]
pub use bootsplash;

#[cfg(feature = "graphics")]
pub use graphics;
