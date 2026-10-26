use timezone::{AVAILABLE_ZONE_IDS, CivilDateTime, DATABASE_VERSION, Error, LocalResolution, Zone};

fn civil(value: &str) -> CivilDateTime {
    value.parse().unwrap()
}
#[cfg(feature = "iana-data")]
fn utc(value: &str) -> i128 {
    match Zone::UTC.local_to_utc(civil(value)).unwrap() {
        LocalResolution::Unique(nanos) => nanos,
        other => panic!("UTC must be unique: {other:?}"),
    }
}

#[test]
fn utc_is_always_available_without_guessing_unknown_zones() {
    for id in ["UTC", "Etc/UTC"] {
        let zone = Zone::get(id).unwrap();
        assert_eq!(zone.id(), "UTC");
        let local = zone.utc_to_local(-1).unwrap();
        assert_eq!(local.datetime(), civil("1969-12-31T23:59:59.999999999"));
        assert_eq!(local.offset_seconds(), 0);
        assert_eq!(local.abbreviation(), "UTC");
        assert!(!local.is_dst());
        assert_eq!(local.utc_nanoseconds(), -1);
        assert_eq!(local.zone_id(), "UTC");
    }
    for id in [
        "",
        "europe/paris",
        "Mars/Olympus",
        "../Europe/Paris",
        "/etc/localtime",
    ] {
        assert_eq!(Zone::get(id).unwrap_err(), Error::UnavailableZone);
    }
    assert!(AVAILABLE_ZONE_IDS.contains(&"UTC"));
}

#[test]
fn extreme_epochs_and_civil_boundaries_are_checked() {
    for nanos in [i128::MIN, i128::MAX] {
        assert_eq!(
            Zone::UTC.utc_to_local(nanos).unwrap_err(),
            Error::OutOfRange
        );
    }
    assert_eq!(
        Zone::UTC
            .local_to_utc(civil("-009999-01-01T00:00:00"))
            .unwrap_err(),
        Error::OutOfRange
    );
}

#[cfg(not(feature = "zone-europe-paris"))]
#[test]
fn omitted_zone_requires_explicit_fallback() {
    assert_eq!(
        Zone::get("Europe/Paris").unwrap_err(),
        Error::UnavailableZone
    );
    assert_eq!(
        Zone::UTC.utc_to_local(0).unwrap().datetime(),
        civil("1970-01-01T00:00:00")
    );
}

#[cfg(not(feature = "iana-data"))]
#[test]
fn default_build_has_no_iana_database() {
    assert_eq!(DATABASE_VERSION, None);
}

#[cfg(feature = "iana-data")]
#[test]
fn generated_database_version_is_exposed() {
    assert_eq!(DATABASE_VERSION, Some("2026e"));
}

#[cfg(feature = "iana-data")]
#[test]
fn custom_static_iana_zone_can_be_reused_without_allocating_or_aliasing_ids() {
    static LONDON: timezone::ZoneRules = jiff::tz::get!("Europe/London");
    let zone = Zone::from_static(&LONDON).unwrap();
    assert_eq!(zone.id(), "Europe/London");
    assert_eq!(
        zone.utc_to_local(utc("2024-07-01T00:00:00"))
            .unwrap()
            .offset_seconds(),
        3600
    );
    static FIXED: timezone::ZoneRules = jiff::tz::TimeZone::fixed(jiff::tz::Offset::constant(1));
    assert_eq!(
        Zone::from_static(&FIXED).unwrap_err(),
        Error::UnavailableZone
    );
}

#[cfg(feature = "zone-europe-paris")]
#[test]
fn paris_spring_transition_has_no_local_0230() {
    let zone = Zone::get("Europe/Paris").unwrap();
    let before = zone
        .utc_to_local(utc("2024-03-31T00:59:59.999999999"))
        .unwrap();
    let after = zone.utc_to_local(utc("2024-03-31T01:00:00")).unwrap();
    assert_eq!(before.datetime(), civil("2024-03-31T01:59:59.999999999"));
    assert_eq!(before.offset_seconds(), 3600);
    assert_eq!(after.datetime(), civil("2024-03-31T03:00:00"));
    assert_eq!(after.offset_seconds(), 7200);
    assert_eq!(
        zone.local_to_utc(civil("2024-03-31T02:30:00")).unwrap(),
        LocalResolution::Nonexistent {
            before_offset_seconds: 3600,
            after_offset_seconds: 7200
        }
    );
}

#[cfg(feature = "zone-europe-paris")]
#[test]
fn paris_autumn_overlap_returns_ordered_distinct_candidates() {
    let zone = Zone::get("Europe/Paris").unwrap();
    let local = civil("2024-10-27T02:30:00.123456789");
    let earlier = utc("2024-10-27T00:30:00.123456789");
    let later = utc("2024-10-27T01:30:00.123456789");
    assert_eq!(
        zone.local_to_utc(local).unwrap(),
        LocalResolution::Ambiguous { earlier, later }
    );
    for nanos in [earlier, later] {
        assert_eq!(zone.utc_to_local(nanos).unwrap().datetime(), local);
    }
    assert_eq!(zone.utc_to_local(earlier).unwrap().abbreviation(), "CEST");
    assert_eq!(zone.utc_to_local(later).unwrap().abbreviation(), "CET");
}

#[cfg(feature = "zone-europe-paris")]
#[test]
fn paris_historical_and_recurring_future_rules_are_used() {
    let zone = Zone::get("Europe/Paris").unwrap();
    for (date, offset, dst, abbr) in [
        ("1890-01-01T00:00:00", 561, false, "LMT"),
        ("2024-01-15T12:00:00", 3600, false, "CET"),
        ("2024-07-15T12:00:00", 7200, true, "CEST"),
        ("2200-01-15T12:00:00", 3600, false, "CET"),
        ("2200-07-15T12:00:00", 7200, true, "CEST"),
    ] {
        let local = zone.utc_to_local(utc(date)).unwrap();
        assert_eq!(local.offset_seconds(), offset);
        assert_eq!(local.is_dst(), dst);
        assert_eq!(local.abbreviation(), abbr);
    }
}

#[cfg(feature = "zone-europe-paris")]
#[test]
fn utc_to_local_crosses_midnight_and_year_without_changing_the_instant() {
    let zone = Zone::get("Europe/Paris").unwrap();
    let nanos = utc("2024-12-31T23:30:00.000000001");
    let local = zone.utc_to_local(nanos).unwrap();
    assert_eq!(local.datetime(), civil("2025-01-01T00:30:00.000000001"));
    assert_eq!(
        zone.local_to_utc(local.datetime()).unwrap(),
        LocalResolution::Unique(nanos)
    );
}

#[cfg(feature = "zone-america-new-york")]
#[test]
fn westward_conversion_can_cross_to_the_previous_year() {
    let zone = Zone::get("America/New_York").unwrap();
    let nanos = utc("2025-01-01T01:00:00");
    assert_eq!(
        zone.utc_to_local(nanos).unwrap().datetime(),
        civil("2024-12-31T20:00:00")
    );
}

#[cfg(all(feature = "zone-asia-kathmandu", feature = "zone-asia-kolkata"))]
#[test]
fn non_hour_offsets_and_historical_changes_are_preserved() {
    let kathmandu = Zone::get("Asia/Kathmandu").unwrap();
    let kolkata = Zone::get("Asia/Kolkata").unwrap();
    let nanos = utc("2024-01-01T00:00:00");
    assert_eq!(
        kathmandu.utc_to_local(nanos).unwrap().offset_seconds(),
        20700
    );
    assert_eq!(
        kathmandu.utc_to_local(nanos).unwrap().datetime(),
        civil("2024-01-01T05:45:00")
    );
    assert_eq!(kolkata.utc_to_local(nanos).unwrap().offset_seconds(), 19800);
    assert_eq!(
        kathmandu
            .utc_to_local(utc("1985-01-01T00:00:00"))
            .unwrap()
            .offset_seconds(),
        19800
    );
    assert_eq!(
        kathmandu
            .local_to_utc(civil("1986-01-01T00:05:00"))
            .unwrap(),
        LocalResolution::Nonexistent {
            before_offset_seconds: 19800,
            after_offset_seconds: 20700
        }
    );
}

#[cfg(feature = "zone-australia-lord-howe")]
#[test]
fn southern_hemisphere_half_hour_dst_gaps_and_overlaps_are_explicit() {
    let zone = Zone::get("Australia/Lord_Howe").unwrap();
    assert_eq!(
        zone.local_to_utc(civil("2024-10-06T02:15:00")).unwrap(),
        LocalResolution::Nonexistent {
            before_offset_seconds: 37800,
            after_offset_seconds: 39600
        }
    );
    assert_eq!(
        zone.local_to_utc(civil("2024-04-07T01:45:00")).unwrap(),
        LocalResolution::Ambiguous {
            earlier: utc("2024-04-06T14:45:00"),
            later: utc("2024-04-06T15:15:00")
        }
    );
}

#[test]
fn every_registered_zone_resolves_and_accepts_timestamp_boundaries() {
    for id in AVAILABLE_ZONE_IDS {
        let zone = Zone::get(id).unwrap();
        assert_eq!(zone.id(), *id);
        for timestamp in [jiff::Timestamp::MIN, jiff::Timestamp::MAX] {
            let nanos = timestamp.as_nanosecond();
            let local = zone.utc_to_local(nanos).unwrap();
            assert_eq!(local.utc_nanoseconds(), nanos);
            assert_eq!(
                zone.local_to_utc(local.datetime()).unwrap(),
                LocalResolution::Unique(nanos)
            );
        }
    }
}
