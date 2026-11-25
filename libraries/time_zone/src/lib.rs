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
//! string into a `Copy` value of 32 bytes that owns its abbreviations; each of
//! the two transition rules is packed into one 32-bit [`Rule`] value. All
//! invalid strings return an explicit [`ParseError`].
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
//! With a region feature (`iana-europe`, `iana-america` and the others, which
//! combine) or `iana-all`, generated tables map IANA identifiers such as
//! `Europe/Paris` to their POSIX TZ string; see the `iana` module. Without a
//! feature, no table is compiled.
//!
//! Limit: a POSIX TZ string holds only the current rules of a zone, not its
//! history. Instants far in the past or future can differ from the full IANA
//! database by the rule changes in between. The current rules of some zones are
//! also not expressible in POSIX form, and their tzdata POSIX footer is an
//! approximation: America/Havana (Cuba keeps -04 until 2027, while the footer
//! says -05 with United States rules), Asia/Gaza and Asia/Hebron, and
//! Africa/Casablanca and Africa/El_Aaiun (Morocco during Ramadan, footer
//! `<+00>0`).
//!
//! ```
//! use time_zone::TimeZone;
//!
//! let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
//! let local = paris.from_utc(1_721_044_800); // 2024-07-15T12:00:00Z
//! assert_eq!(local.offset_seconds, 7200);
//! assert_eq!(local.abbreviation, "CEST");
//! assert!(local.daylight_saving);
//! assert_eq!((local.date_time.hour, local.date_time.minute), (14, 0));
//! ```
#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

mod civil;
#[cfg(any(
    feature = "iana-africa",
    feature = "iana-america",
    feature = "iana-antarctica",
    feature = "iana-arctic",
    feature = "iana-asia",
    feature = "iana-atlantic",
    feature = "iana-australia",
    feature = "iana-etc",
    feature = "iana-europe",
    feature = "iana-indian",
    feature = "iana-pacific"
))]
mod generated;
#[cfg(any(
    feature = "iana-africa",
    feature = "iana-america",
    feature = "iana-antarctica",
    feature = "iana-arctic",
    feature = "iana-asia",
    feature = "iana-atlantic",
    feature = "iana-australia",
    feature = "iana-etc",
    feature = "iana-europe",
    feature = "iana-indian",
    feature = "iana-pacific"
))]
pub mod iana;
mod parse;
mod rule;
mod zone;

pub use civil::DateTime;
pub use parse::ParseError;
pub use rule::{Rule, RuleKind};
pub use zone::{ABBREVIATION_MAX, Local, LocalToUtc, TimeZone};
