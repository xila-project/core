//! Tests for the generated IANA zone name table.
//!
//! Run with table features, for example:
//! `cargo test -p xila-time-zone --features iana-all`.
#![cfg(any(
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

extern crate std;

use time_zone::{TimeZone, iana};

#[test]
fn version_is_recorded() {
    assert!(
        iana::TZDATA_VERSION.starts_with("20"),
        "{}",
        iana::TZDATA_VERSION
    );
}

fn region(name: &str) -> Option<&str> {
    name.split_once('/').map(|(region, _)| region)
}

#[test]
fn names_are_sorted_within_each_region_and_looked_up() {
    let mut previous: Option<&str> = None;
    for index in 0..iana::zone_count() {
        let name = iana::zone_name(index).unwrap();
        if let Some(previous) = previous
            && region(previous) == region(name)
            && region(name).is_some()
        {
            assert!(previous < name, "{previous} !< {name}");
        }
        previous = Some(name);
    }
}

#[test]
fn names_are_unique_and_looked_up() {
    let count = iana::zone_count();
    let mut names = std::collections::BTreeSet::new();
    for index in 0..count {
        let name = iana::zone_name(index).unwrap();
        assert!(names.insert(name), "{name} is listed twice");
        assert!(iana::contains(name), "{name}");
        assert!(!iana::time_zone_string(name).unwrap().is_empty());
    }
    assert!(iana::zone_name(count).is_none());
    assert!(!iana::contains("Not/AZone"));
    assert!(!iana::contains(""));
    assert_eq!(iana::time_zone_string("Not/AZone"), None);
    assert!(TimeZone::from_iana_name("Not/AZone").is_none());
}

/// Each region holds the names with its prefix only.
#[test]
fn regions_follow_the_prefix() {
    let regions = [
        (cfg!(feature = "iana-africa"), "Africa/Abidjan"),
        (cfg!(feature = "iana-america"), "America/New_York"),
        (cfg!(feature = "iana-america"), "EST5EDT"),
        (cfg!(feature = "iana-antarctica"), "Antarctica/Troll"),
        (cfg!(feature = "iana-asia"), "Asia/Kolkata"),
        (cfg!(feature = "iana-atlantic"), "Atlantic/Madeira"),
        (cfg!(feature = "iana-australia"), "Australia/Sydney"),
        (cfg!(feature = "iana-etc"), "Etc/GMT+5"),
        (cfg!(feature = "iana-etc"), "GMT"),
        (cfg!(feature = "iana-europe"), "Europe/Paris"),
        (cfg!(feature = "iana-indian"), "Indian/Maldives"),
        (cfg!(feature = "iana-pacific"), "Pacific/Auckland"),
    ];
    for (enabled, name) in regions {
        assert_eq!(iana::contains(name), enabled, "{name}");
    }
    assert!(!iana::contains("Factory"));
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

#[cfg(feature = "iana-europe")]
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

/// Legacy aliases: `backward` links of tzdata 2026f that `zone.tab` does not
/// list, with the region feature that holds each one.
const LEGACY_ALIASES: &[(bool, &str)] = &[
    (cfg!(feature = "iana-america"), "US/Eastern"),
    (cfg!(feature = "iana-america"), "Canada/Pacific"),
    (cfg!(feature = "iana-asia"), "Asia/Calcutta"),
    (cfg!(feature = "iana-asia"), "Japan"),
    (cfg!(feature = "iana-europe"), "Europe/Kiev"),
    (cfg!(feature = "iana-europe"), "GB"),
    (cfg!(feature = "iana-europe"), "Portugal"),
    (cfg!(feature = "iana-etc"), "Etc/UCT"),
];

/// Listed aliases: `backward` links that `zone.tab` lists for a country. They
/// are in the default tables.
const LISTED_ALIASES: &[(bool, &str)] = &[
    (cfg!(feature = "iana-arctic"), "Arctic/Longyearbyen"),
    (cfg!(feature = "iana-atlantic"), "Atlantic/Reykjavik"),
    (cfg!(feature = "iana-asia"), "Asia/Kuwait"),
    (cfg!(feature = "iana-asia"), "Asia/Muscat"),
    (cfg!(feature = "iana-america"), "America/Nassau"),
    (cfg!(feature = "iana-europe"), "Europe/Amsterdam"),
    (cfg!(feature = "iana-europe"), "Europe/Oslo"),
    (cfg!(feature = "iana-europe"), "Europe/Stockholm"),
];

#[test]
fn legacy_aliases_only_with_the_feature() {
    let aliases = cfg!(feature = "iana-legacy-aliases");
    for (region, name) in LEGACY_ALIASES {
        assert_eq!(iana::contains(name), aliases && *region, "{name}");
        assert_eq!(
            TimeZone::from_iana_name(name).is_some(),
            aliases && *region,
            "{name}"
        );
    }
}

#[test]
fn listed_aliases_by_default() {
    for (region, name) in LISTED_ALIASES {
        assert_eq!(iana::contains(name), *region, "{name}");
    }
    #[cfg(feature = "iana-europe")]
    assert_eq!(
        iana::time_zone_string("Europe/Amsterdam"),
        iana::time_zone_string("Europe/Brussels")
    );
}

#[cfg(all(feature = "iana-legacy-aliases", feature = "iana-all"))]
#[test]
fn an_alias_resolves_to_its_target() {
    assert_eq!(
        iana::time_zone_string("Asia/Calcutta"),
        iana::time_zone_string("Asia/Kolkata")
    );
    assert_eq!(
        iana::time_zone_string("US/Eastern"),
        iana::time_zone_string("America/New_York")
    );
}

#[cfg(feature = "iana-all")]
#[test]
fn all_zones_table() {
    let expected = if cfg!(feature = "iana-legacy-aliases") {
        597
    } else {
        451
    };
    assert_eq!(iana::zone_count(), expected);
    assert_eq!(iana::time_zone_string("Asia/Kolkata"), Some("IST-5:30"));
    assert_eq!(iana::time_zone_string("Etc/GMT+5"), Some("<-05>5"));
    let kolkata = TimeZone::from_iana_name("Asia/Kolkata").unwrap();
    assert_eq!(kolkata.standard_offset_seconds(), 19_800);
    assert!(!kolkata.has_daylight_saving());
}

#[cfg(feature = "iana-europe")]
#[test]
fn europe_region() {
    assert!(iana::contains("Europe/Paris"));
    assert!(iana::contains("Europe/Dublin"));
    assert!(iana::contains("Europe/Kyiv"));
    assert_eq!(
        iana::time_zone_string("Europe/Paris"),
        Some("CET-1CEST,M3.5.0,M10.5.0/3")
    );
}

#[cfg(all(
    feature = "iana-europe",
    not(any(
        feature = "iana-africa",
        feature = "iana-america",
        feature = "iana-antarctica",
        feature = "iana-arctic",
        feature = "iana-asia",
        feature = "iana-atlantic",
        feature = "iana-australia",
        feature = "iana-etc",
        feature = "iana-indian",
        feature = "iana-pacific"
    ))
))]
#[test]
fn europe_region_alone() {
    let expected = if cfg!(feature = "iana-legacy-aliases") {
        75
    } else {
        58
    };
    assert_eq!(iana::zone_count(), expected);
    assert!(!iana::contains("Atlantic/Madeira"));
    assert!(!iana::contains("America/New_York"));
}
