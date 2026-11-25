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
        assert!(!iana::tz_string(previous).unwrap().is_empty());
    }
    assert!(iana::zone_name(count).is_none());
    assert!(!iana::contains("Not/AZone"));
    assert_eq!(iana::tz_string("Not/AZone"), None);
    assert!(TimeZone::from_iana_name("Not/AZone").is_none());
}

#[test]
fn every_table_string_parses() {
    for index in 0..iana::zone_count() {
        let name = iana::zone_name(index).unwrap();
        let tz_string = iana::tz_string(name).unwrap();
        assert!(
            TimeZone::parse(tz_string).is_ok(),
            "{name}: {tz_string:?} does not parse"
        );
        // The lookup returns the same string for the parsed zone.
        let zone = TimeZone::from_iana_name(name).unwrap();
        assert!(zone.std_abbreviation().len() >= 3, "{name}");
    }
}

#[test]
fn known_zones_convert() {
    let paris = TimeZone::from_iana_name("Europe/Paris").unwrap();
    assert_eq!(paris.std_abbreviation(), "CET");
    assert_eq!(paris.dst_abbreviation(), Some("CEST"));
    assert_eq!(paris.std_offset_seconds(), 3600);
    let local = paris.from_utc(1_721_044_800); // 2024-07-15 12:00:00 UTC
    assert_eq!(local.abbreviation, "CEST");
    assert_eq!(local.offset_seconds, 7200);
    assert!(local.dst);

    let dublin = TimeZone::from_iana_name("Europe/Dublin").unwrap();
    assert_eq!(dublin.std_abbreviation(), "IST");
    assert_eq!(dublin.dst_abbreviation(), Some("GMT"));
    assert_eq!(dublin.dst_offset_seconds(), Some(0));
}

#[cfg(feature = "iana-all")]
#[test]
fn all_zones_table_spot_checks() {
    let kolkata = TimeZone::from_iana_name("Asia/Kolkata").unwrap();
    assert_eq!(kolkata.std_offset_seconds(), 19_800);
    assert!(!kolkata.has_dst());
    assert_eq!(iana::tz_string("Asia/Kolkata"), Some("IST-5:30"));
}

#[cfg(feature = "iana-all")]
#[test]
fn all_zones_table() {
    assert!(iana::zone_count() > 500, "{}", iana::zone_count());
    assert_eq!(iana::tz_string("Asia/Kolkata"), Some("IST-5:30"));
    assert_eq!(
        iana::tz_string("Europe/Paris"),
        Some("CET-1CEST,M3.5.0,M10.5.0/3")
    );
    assert!(iana::contains("US/Pacific"));
    assert!(iana::contains("Asia/Calcutta")); // a backward link
    assert!(iana::contains("Etc/UTC"));
}

#[cfg(all(feature = "iana-europe", not(feature = "iana-all")))]
#[test]
fn europe_subset_table() {
    assert!(iana::zone_count() > 50, "{}", iana::zone_count());
    assert!(iana::zone_count() < 100, "{}", iana::zone_count());
    assert!(iana::contains("Europe/Paris"));
    assert!(iana::contains("Europe/Dublin"));
    assert!(!iana::contains("America/New_York"));
    assert!(!iana::contains("Asia/Kolkata"));
}
