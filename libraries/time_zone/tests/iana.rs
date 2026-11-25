//! Tests for the generated IANA zone name table.
//!
//! Run with a table feature, for example:
//! `cargo test -p xila-time-zone --features iana-all`.
#![cfg(any(feature = "iana-all", feature = "iana-europe"))]

use time_zone::{TimeZone, iana};

#[test]
fn version_is_recorded() {
    assert!(
        iana::TZDATA_VERSION.starts_with("20"),
        "{}",
        iana::TZDATA_VERSION
    );
}

#[test]
fn names_are_sorted_unique_and_looked_up() {
    let count = iana::zone_count();
    assert!(count > 0);
    for index in 1..count {
        let previous = iana::zone_name(index - 1).unwrap();
        let current = iana::zone_name(index).unwrap();
        assert!(previous < current, "{previous} !< {current}");
        assert!(iana::contains(previous));
        assert!(!iana::time_zone_string(previous).unwrap().is_empty());
    }
    assert!(iana::zone_name(count).is_none());
    assert!(!iana::contains("Not/AZone"));
    assert_eq!(iana::time_zone_string("Not/AZone"), None);
    assert!(TimeZone::from_iana_name("Not/AZone").is_none());
}

#[test]
fn every_table_string_parses() {
    for index in 0..iana::zone_count() {
        let name = iana::zone_name(index).unwrap();
        let time_zone_string = iana::time_zone_string(name).unwrap();
        assert!(
            TimeZone::parse(time_zone_string).is_ok(),
            "{name}: {time_zone_string:?} does not parse"
        );
        // The lookup returns the same string for the parsed zone.
        let zone = TimeZone::from_iana_name(name).unwrap();
        assert!(zone.standard_abbreviation().len() >= 3, "{name}");
    }
}

#[test]
fn known_zones_convert() {
    let paris = TimeZone::from_iana_name("Europe/Paris").unwrap();
    assert_eq!(paris.standard_abbreviation(), "CET");
    assert_eq!(paris.daylight_saving_abbreviation(), Some("CEST"));
    assert_eq!(paris.standard_offset_seconds(), 3600);
    let local = paris.from_utc(1_721_044_800); // 2024-07-15 12:00:00 UTC
    assert_eq!(local.abbreviation, "CEST");
    assert_eq!(local.offset_seconds, 7200);
    assert!(local.daylight_saving);

    let dublin = TimeZone::from_iana_name("Europe/Dublin").unwrap();
    assert_eq!(dublin.standard_abbreviation(), "IST");
    assert_eq!(dublin.daylight_saving_abbreviation(), Some("GMT"));
    assert_eq!(dublin.daylight_saving_offset_seconds(), Some(0));
}

#[cfg(feature = "iana-all")]
#[test]
fn all_zones_table() {
    assert!(iana::zone_count() > 500, "{}", iana::zone_count());
    assert_eq!(iana::time_zone_string("Asia/Kolkata"), Some("IST-5:30"));
    assert_eq!(
        iana::time_zone_string("Europe/Paris"),
        Some("CET-1CEST,M3.5.0,M10.5.0/3")
    );
    assert!(iana::contains("US/Pacific"));
    assert!(iana::contains("Asia/Calcutta")); // a backward link
    assert!(iana::contains("Etc/UTC"));
    let kolkata = TimeZone::from_iana_name("Asia/Kolkata").unwrap();
    assert_eq!(kolkata.standard_offset_seconds(), 19_800);
    assert!(!kolkata.has_daylight_saving());
}

#[cfg(all(feature = "iana-europe", not(feature = "iana-all")))]
#[test]
fn europe_subset_table() {
    // The subset holds the Europe/ zones plus the European zones outside that
    // prefix and the backward links to them.
    assert!(iana::zone_count() > 50, "{}", iana::zone_count());
    assert!(iana::zone_count() < 200, "{}", iana::zone_count());
    assert!(iana::contains("Europe/Paris"));
    assert!(iana::contains("Europe/Dublin"));
    assert!(iana::contains("Atlantic/Canary"));
    assert!(iana::contains("Atlantic/Madeira"));
    assert!(iana::contains("Atlantic/Reykjavik"));
    assert!(iana::contains("Arctic/Longyearbyen"));
    assert!(iana::contains("Asia/Nicosia"));
    assert!(iana::contains("Iceland")); // a backward link
    assert!(iana::contains("Portugal"));
    assert!(iana::contains("Poland"));
    assert!(iana::contains("GB"));
    assert!(!iana::contains("America/New_York"));
    assert!(!iana::contains("Asia/Kolkata"));
}
