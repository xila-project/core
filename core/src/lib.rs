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
//!   [`drivers_neural_network`] (`neural_network`), [`bootsplash`] (`bootsplash`), and
//!   [`time_zone`] and [`local_time`] (`time_zone`; the IANA zone name tables are
//!   features of `xila-time-zone` itself, such as `iana-europe` or `iana-all`).

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

/// Time zone and daylight saving conversion with POSIX TZ rule strings
/// (`xila-time-zone`). With the region features of `xila-time-zone`, such as
/// `iana-europe` or `iana-all`, the `time_zone::iana` module maps IANA zone
/// names to their POSIX TZ strings.
#[cfg(feature = "time_zone")]
pub use time_zone;

/// The system time zone setting (`xila-local-time`): its storage in
/// `/configuration/system/time_zone.json`, its loading at start-up and its
/// publication to every task as the POSIX `TZ` environment variable.
#[cfg(feature = "time_zone")]
pub use local_time;

#[cfg(feature = "graphics")]
pub use graphics;
