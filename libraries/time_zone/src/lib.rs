//! Time zone and daylight saving time conversion with POSIX TZ rule strings.
//!
//! This library stays outside the mandatory kernel clock (xila-project/core#463):
//! it converts between UTC and local civil time, and it never touches a clock.
//! It is `no_std` and performs no allocation, so it runs on devices with
//! 256 KB of RAM.
//!
//! A time zone is described by one POSIX TZ string, the `TZ` format of POSIX
//! `tzset`: `std offset [dst [offset] [,rule,rule]]`, for example
//! `CET-1CEST,M3.5.0,M10.5.0/3` for `Europe/Paris`. [`TimeZone::parse`] turns the
//! string into a small `Copy` value; the two transition rules are packed into
//! one 32-bit [`Rule`] each. All errors are explicit, see [`ParseError`].
//!
//! Conversions use plain `i64` seconds since the Unix epoch
//! (1970-01-01T00:00:00Z), not the `Timestamp` type of the `time` subsystem, so
//! that this library stays independent of the kernel clock. Callers that hold a
//! `time::Timestamp` convert it with `Timestamp::try_as_seconds`.
//!
//! - UTC to local: [`TimeZone::from_utc`] returns the local date and time, the
//!   offset in seconds, the zone abbreviation and whether daylight saving time
//!   applies.
//! - Local to UTC: [`TimeZone::to_utc`] returns an explicit [`LocalToUtc`] result:
//!   a unique instant, both instants of a repeated local time (autumn), or a
//!   nonexistent local time (spring gap). It never picks one silently.
//!
//! With the `iana-all` or `iana-europe` Cargo features, a generated table maps
//! IANA identifiers such as `Europe/Paris` to their POSIX TZ string; see the
//! `iana` module. Without a feature, no table is compiled.
//!
//! Limit: a POSIX TZ string holds only the current rules of a zone, not its
//! history. Instants far in the past or future can differ from the full IANA
//! database by the rule changes in between, and zones whose future rules are
//! not expressible in POSIX form (for example Morocco during Ramadan) use the
//! approximation that the tzdata POSIX footer carries.
//!
//! ```
//! use time_zone::TimeZone;
//!
//! let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
//! let local = paris.from_utc(1_721_044_800); // 2024-07-15T12:00:00Z
//! assert_eq!(local.offset_seconds, 7200);
//! assert_eq!(local.abbreviation, "CEST");
//! assert!(local.dst);
//! assert_eq!((local.date_time.hour, local.date_time.minute), (14, 0));
//! ```
#![no_std]
#![forbid(unsafe_code)]

mod civil;
mod generated;
#[cfg(any(feature = "iana-all", feature = "iana-europe"))]
pub mod iana;
mod parse;
mod rule;
mod zone;

pub use civil::DateTime;
pub use parse::ParseError;
pub use rule::{Rule, RuleKind};
pub use zone::{Local, LocalToUtc, TimeZone};
