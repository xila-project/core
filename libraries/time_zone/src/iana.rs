//! Optional IANA zone name tables.
//!
//! The tables map IANA identifiers such as `Europe/Paris` to the POSIX TZ
//! string of the zone. They are generated from the IANA time zone database; the
//! version of the database is in [`TZDATA_VERSION`]. One table per region is
//! compiled with its Cargo feature, and the features combine:
//!
//! | Feature | Names |
//! | --- | --- |
//! | none (default) | no names: [`zone_count`] is 0 and every lookup fails |
//! | `iana-africa`, `iana-america`, `iana-antarctica`, `iana-arctic`, `iana-asia`, `iana-atlantic`, `iana-australia`, `iana-europe`, `iana-indian`, `iana-pacific` | the names with that prefix, for example `Europe/Paris` |
//! | `iana-etc` | the fixed offset zones `Etc/...` and `GMT` |
//! | `iana-all` | every region |
//!
//! A region holds the names with its prefix only: `Atlantic/Madeira` is in
//! `iana-atlantic`, not in `iana-europe`. The four canonical names without a
//! prefix, `CST6CDT`, `EST5EDT`, `MST7MDT` and `PST8PDT`, are in
//! `iana-america`. `Factory` is in no table.
//!
//! By default, the tables hold the canonical names and the listed aliases:
//! the `backward` links that tzdata `zone.tab` lists for a country, such as
//! `Europe/Amsterdam`, `Europe/Oslo` or `Atlantic/Reykjavik` (zones that
//! tzdata merged into another one), so that every country keeps its names.
//! With `iana-legacy-aliases`, each enabled region also holds the other
//! `backward` links, for example `US/Eastern`, `Japan` or `Asia/Calcutta`. An
//! alias is in the region of its prefix, or, without a region prefix, in the
//! region of the zone that it names (`US/Eastern` is in `iana-america`). An
//! alias row is its name and the index of the TZ string of the zone that it
//! names, shared with that zone.
//!
//! [`zone_name`] lists the enabled region tables in alphabetical order of
//! feature name (`iana-africa` through `iana-pacific`, with `iana-etc` between
//! `iana-australia` and `iana-europe`), with the names of each table sorted,
//! aliases included. Zones of a region that share a TZ string share it in one
//! string blob.
//!
//! ```
//! # #[cfg(feature = "iana-europe")]
//! # {
//! use time_zone::{iana, TimeZone};
//!
//! let paris = TimeZone::from_iana_name("Europe/Paris").unwrap();
//! assert_eq!(paris.standard_abbreviation(), "CET");
//! assert_eq!(
//!     iana::time_zone_string("Europe/Dublin"),
//!     Some("IST-1GMT0,M10.5.0,M3.5.0/1")
//! );
//! assert!(TimeZone::from_iana_name("Not/AZone").is_none());
//! # }
//! ```

use crate::TimeZone;
use crate::generated;
use core::cmp::Ordering;

/// The version of the IANA time zone database that the tables were generated
/// from, for example `2026f`.
pub const TZDATA_VERSION: &str = generated::TZDATA_VERSION;

/// One generated table: a sorted name blob and a TZ string blob, both of
/// `\0` terminated strings.
pub(crate) struct Table {
    /// The names, sorted, each ended by `\0`.
    pub(crate) names: &'static str,
    /// The start of each name in `names`, plus the end of the blob.
    pub(crate) name_offsets: &'static [u16],
    /// The index in `string_offsets` of the TZ string of each name.
    pub(crate) zone_string: &'static [u8],
    /// The TZ strings, each ended by `\0`.
    pub(crate) strings: &'static str,
    /// The start of each TZ string in `strings`, plus the end of the blob.
    pub(crate) string_offsets: &'static [u16],
}

impl Table {
    fn count(&self) -> usize {
        self.name_offsets.len() - 1
    }

    fn name(&self, index: usize) -> Option<&'static str> {
        let start = usize::from(*self.name_offsets.get(index)?);
        let end = usize::from(*self.name_offsets.get(index + 1)?);
        self.names.get(start..end - 1)
    }

    fn time_zone_string(&self, index: usize) -> Option<&'static str> {
        let string = usize::from(*self.zone_string.get(index)?);
        let start = usize::from(*self.string_offsets.get(string)?);
        let end = usize::from(*self.string_offsets.get(string + 1)?);
        self.strings.get(start..end - 1)
    }

    /// Binary search for a name. The lookup compares bytes in a loop so that
    /// it needs no `memcmp` call; some freestanding targets do not provide one.
    fn lookup(&self, name: &str) -> Option<usize> {
        let mut low = 0;
        let mut high = self.count();
        while low < high {
            let middle = (low + high) / 2;
            match compare(self.name(middle)?.as_bytes(), name.as_bytes()) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => return Some(middle),
            }
        }
        None
    }
}

/// The tables of the enabled features, in the order of [`zone_name`].
const TABLES: &[&Table] = &[
    #[cfg(all(feature = "iana-africa", not(feature = "iana-legacy-aliases")))]
    &generated::africa::TABLE,
    #[cfg(all(feature = "iana-africa", feature = "iana-legacy-aliases"))]
    &generated::africa::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-america", not(feature = "iana-legacy-aliases")))]
    &generated::america::TABLE,
    #[cfg(all(feature = "iana-america", feature = "iana-legacy-aliases"))]
    &generated::america::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-antarctica", not(feature = "iana-legacy-aliases")))]
    &generated::antarctica::TABLE,
    #[cfg(all(feature = "iana-antarctica", feature = "iana-legacy-aliases"))]
    &generated::antarctica::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-arctic", not(feature = "iana-legacy-aliases")))]
    &generated::arctic::TABLE,
    #[cfg(all(feature = "iana-arctic", feature = "iana-legacy-aliases"))]
    &generated::arctic::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-asia", not(feature = "iana-legacy-aliases")))]
    &generated::asia::TABLE,
    #[cfg(all(feature = "iana-asia", feature = "iana-legacy-aliases"))]
    &generated::asia::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-atlantic", not(feature = "iana-legacy-aliases")))]
    &generated::atlantic::TABLE,
    #[cfg(all(feature = "iana-atlantic", feature = "iana-legacy-aliases"))]
    &generated::atlantic::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-australia", not(feature = "iana-legacy-aliases")))]
    &generated::australia::TABLE,
    #[cfg(all(feature = "iana-australia", feature = "iana-legacy-aliases"))]
    &generated::australia::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-etc", not(feature = "iana-legacy-aliases")))]
    &generated::etc::TABLE,
    #[cfg(all(feature = "iana-etc", feature = "iana-legacy-aliases"))]
    &generated::etc::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-europe", not(feature = "iana-legacy-aliases")))]
    &generated::europe::TABLE,
    #[cfg(all(feature = "iana-europe", feature = "iana-legacy-aliases"))]
    &generated::europe::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-indian", not(feature = "iana-legacy-aliases")))]
    &generated::indian::TABLE,
    #[cfg(all(feature = "iana-indian", feature = "iana-legacy-aliases"))]
    &generated::indian::TABLE_WITH_LEGACY_ALIASES,
    #[cfg(all(feature = "iana-pacific", not(feature = "iana-legacy-aliases")))]
    &generated::pacific::TABLE,
    #[cfg(all(feature = "iana-pacific", feature = "iana-legacy-aliases"))]
    &generated::pacific::TABLE_WITH_LEGACY_ALIASES,
];

/// The number of zone names in the enabled tables.
pub fn zone_count() -> usize {
    TABLES.iter().map(|table| table.count()).sum()
}

/// The zone name at an index, for 0 to [`zone_count`].
pub fn zone_name(index: usize) -> Option<&'static str> {
    let mut index = index;
    for table in TABLES {
        if index < table.count() {
            return table.name(index);
        }
        index -= table.count();
    }
    None
}

/// Whether the enabled tables have a zone name.
pub fn contains(name: &str) -> bool {
    time_zone_string(name).is_some()
}

/// The POSIX TZ string of an IANA zone name, if the enabled tables have the
/// name.
pub fn time_zone_string(name: &str) -> Option<&'static str> {
    TABLES.iter().find_map(|table| {
        let index = table.lookup(name)?;
        table.time_zone_string(index)
    })
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

#[cfg(test)]
mod tests {
    #[test]
    fn lookups_follow_the_enabled_regions() {
        let europe = cfg!(feature = "iana-europe");
        assert_eq!(super::contains("Europe/Paris"), europe);
        assert_eq!(super::time_zone_string("Europe/Paris").is_some(), europe);
        if super::TABLES.is_empty() {
            assert_eq!(super::zone_count(), 0);
            assert_eq!(super::zone_name(0), None);
        }
    }

    #[test]
    fn each_table_is_strictly_sorted() {
        for table in super::TABLES {
            let mut previous: Option<&str> = None;
            for index in 0..table.count() {
                let name = table.name(index).unwrap();
                if let Some(previous) = previous {
                    assert!(previous < name, "{previous} !< {name}");
                }
                previous = Some(name);
            }
        }
    }

    #[test]
    fn utc_const_matches_parse() {
        assert_eq!(
            crate::TimeZone::utc(),
            crate::TimeZone::parse("UTC0").unwrap()
        );
    }
}
