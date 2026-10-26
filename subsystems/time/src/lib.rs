//! System-owned UTC and monotonic clocks. UTC is invalid until explicitly seeded.
//!
//! Native/browser runtimes bootstrap once through their typed `TimeDevice` source.
//! Their host clock is trusted as the epoch supplier, not certified as accurate;
//! subsequent host clock changes require explicit resynchronization.
//! A target with no RTC can start the clock before any shell or network service:
//!
//! ```
//! use time::{ClockSource, Manager, RealtimeSample, Timestamp};
//! use core::time::Duration;
//! let clock = Manager::unsynchronized();
//! // Runtime policy validates the manual value (or obtains it from an RTC/network).
//! clock.set_realtime(RealtimeSample {
//!     timestamp: Timestamp::from_nanos(1_700_000_000_000_000_000),
//!     source: ClockSource::Manual,
//!     resolution: Duration::from_secs(1),
//!     accuracy: None,
//! }).unwrap();
//! ```
//!
//! The API does not implement a synchronization protocol, timezone, persistence,
//! authentication or automatic source fallback. Unsynchronized reads fail; current
//! unsigned filesystem and WASI formats also reject pre-epoch/out-of-range UTC.

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
