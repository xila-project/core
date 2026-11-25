//! Optional IANA zone name table.
//!
//! The table maps IANA identifiers such as `Europe/Paris` to the POSIX TZ
//! string of the zone. It is generated from the IANA time zone database; the
//! version of the database is in [`TZDATA_VERSION`]. The table is compiled only
//! with a Cargo feature:
//!
//! | Feature | Table |
//! | --- | --- |
//! | none (default) | no table |
//! | `iana-europe` | the Europe subset |
//! | `iana-all` | all zones |
//!
//! `iana-all` wins when both features are enabled. The names are sorted for
//! binary search, and zones that share a TZ string share it in one string blob.
//!
//! ```
//! use time_zone::{iana, TimeZone};
//!
//! let paris = TimeZone::from_iana_name("Europe/Paris").unwrap();
//! assert_eq!(paris.standard_abbreviation(), "CET");
//! assert_eq!(
//!     iana::time_zone_string("Europe/Dublin"),
//!     Some("IST-1GMT0,M10.5.0,M3.5.0/1")
//! );
//! assert!(TimeZone::from_iana_name("Not/AZone").is_none());
//! ```

#[cfg(feature = "iana-all")]
use crate::generated::all as table;
#[cfg(all(feature = "iana-europe", not(feature = "iana-all")))]
use crate::generated::europe as table;

use crate::TimeZone;
use core::cmp::Ordering;

/// The version of the IANA time zone database that the table was generated
/// from, for example `2026f`.
pub const TZDATA_VERSION: &str = table::TZDATA_VERSION;

/// The number of zone names in the table.
pub fn zone_count() -> usize {
    table::NAME_OFFSETS.len() - 1
}

/// The zone name at an index, for 0 to [`zone_count`]. Names are sorted.
pub fn zone_name(index: usize) -> Option<&'static str> {
    let start = *table::NAME_OFFSETS.get(index)? as usize;
    let end = *table::NAME_OFFSETS.get(index + 1)? as usize;
    Some(&table::NAMES[start..end - 1])
}

/// Whether the table has a zone name.
pub fn contains(name: &str) -> bool {
    lookup(name).is_some()
}

/// The POSIX TZ string of an IANA zone name, if the table has the name.
pub fn time_zone_string(name: &str) -> Option<&'static str> {
    let index = lookup(name)?;
    let string_index = table::ZONE_STRING[index] as usize;
    let start = *table::STRING_OFFSETS.get(string_index)? as usize;
    let end = *table::STRING_OFFSETS.get(string_index + 1)? as usize;
    Some(&table::STRINGS[start..end - 1])
}

/// Binary search for a zone name in the sorted name blob. The lookup compares
/// bytes in a loop so that it needs no `memcmp` call; some freestanding targets
/// do not provide one.
fn lookup(name: &str) -> Option<usize> {
    let mut low = 0;
    let mut high = zone_count();
    while low < high {
        let middle = (low + high) / 2;
        match compare(zone_name(middle)?.as_bytes(), name.as_bytes()) {
            Ordering::Less => low = middle + 1,
            Ordering::Greater => high = middle,
            Ordering::Equal => return Some(middle),
        }
    }
    None
}

/// Compare two byte slices without a `memcmp` call.
fn compare(left: &[u8], right: &[u8]) -> Ordering {
    let shared = left.len().min(right.len());
    for index in 0..shared {
        match left[index].cmp(&right[index]) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    left.len().cmp(&right.len())
}

impl TimeZone {
    /// Parse the POSIX TZ string of an IANA zone name, for example
    /// `Europe/Paris`. Returns None when the table does not have the name.
    ///
    /// Every string in the generated table parses, so the parse error path is
    /// not reachable here; it is still handled to keep the function total.
    pub fn from_iana_name(name: &str) -> Option<TimeZone> {
        TimeZone::parse(time_zone_string(name)?).ok()
    }
}
