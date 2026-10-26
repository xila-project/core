//! Reusable UTC/civil conversion, independent of locale, formatting and clock state.
//!
//! UTC is always available; named IANA zones require explicit Cargo features.
//! Missing zones return an error. No system timezone is detected and no fallback
//! is selected implicitly. Static conversion uses no allocator or system calls.
//! See the crate README for the zone list, database version/update process,
//! supported range and measured footprint.
//!
//! ```
//! use timezone::{CivilDateTime, LocalResolution, Zone};
//! let local = Zone::UTC.utc_to_local(-1)?;
//! assert_eq!(local.datetime(), "1969-12-31T23:59:59.999999999".parse::<CivilDateTime>().unwrap());
//! assert_eq!(Zone::UTC.local_to_utc(local.datetime())?, LocalResolution::Unique(-1));
//! # Ok::<(), timezone::Error>(())
//! ```
#![no_std]

mod error;
mod zone;

pub use error::{Error, Result};
pub use jiff::{civil::DateTime as CivilDateTime, tz::TimeZone as ZoneRules};
pub use zone::{AVAILABLE_ZONE_IDS, LocalResolution, LocalTime, Zone};

/// Version of the pinned IANA data used by the static zone generator.
/// None when no IANA data feature is enabled (UTC-only).
#[cfg(feature = "iana-data")]
pub const DATABASE_VERSION: Option<&str> = Some(env!("XILA_TZDB_VERSION"));
#[cfg(not(feature = "iana-data"))]
pub const DATABASE_VERSION: Option<&str> = None;
