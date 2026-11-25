//! Conversions between UTC and local time for zones that cover the northern
//! and southern hemisphere, half hour offsets, no daylight saving time and
//! negative daylight saving offsets, including the exact transition instants
//! and the ambiguous and nonexistent local times.

use time_zone::{DateTime, LocalToUtc, TimeZone};

const fn dt(year: i64, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> DateTime {
    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

#[test]
fn europe_paris() {
    let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();

    // Winter: 2024-01-15 12:00:00 UTC is 13:00:00 CET.
    let local = paris.from_utc(1_705_320_000);
    assert_eq!(local.date_time, dt(2024, 1, 15, 13, 0, 0));
    assert_eq!(local.offset_seconds, 3600);
    assert_eq!(local.abbreviation, "CET");
    assert!(!local.dst);

    // Summer: 2024-07-15 12:00:00 UTC is 14:00:00 CEST.
    let local = paris.from_utc(1_721_044_800);
    assert_eq!(local.date_time, dt(2024, 7, 15, 14, 0, 0));
    assert_eq!(local.offset_seconds, 7200);
    assert_eq!(local.abbreviation, "CEST");
    assert!(local.dst);

    // The epoch itself uses the current rules: 1970-01-01 00:00:00 UTC is
    // 01:00:00 CET. A POSIX TZ string has no history.
    let local = paris.from_utc(0);
    assert_eq!(local.date_time, dt(1970, 1, 1, 1, 0, 0));
    assert_eq!(local.offset_seconds, 3600);

    // Spring forward 2024: at 2024-03-31 01:00:00 UTC the clocks jump from
    // 02:00:00 CET to 03:00:00 CEST.
    let local = paris.from_utc(1_711_846_799); // 00:59:59 UTC
    assert_eq!(local.date_time, dt(2024, 3, 31, 1, 59, 59));
    assert_eq!(local.offset_seconds, 3600);
    assert_eq!(local.abbreviation, "CET");
    let local = paris.from_utc(1_711_846_800); // 01:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 3, 31, 3, 0, 0));
    assert_eq!(local.offset_seconds, 7200);
    assert_eq!(local.abbreviation, "CEST");

    // Autumn back 2024: at 2024-10-27 01:00:00 UTC the clocks go from
    // 03:00:00 CEST to 02:00:00 CET.
    let local = paris.from_utc(1_729_990_799); // 00:59:59 UTC
    assert_eq!(local.date_time, dt(2024, 10, 27, 2, 59, 59));
    assert_eq!(local.offset_seconds, 7200);
    assert_eq!(local.abbreviation, "CEST");
    let local = paris.from_utc(1_729_990_800); // 01:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 10, 27, 2, 0, 0));
    assert_eq!(local.offset_seconds, 3600);
    assert_eq!(local.abbreviation, "CET");

    // Local 2024-03-31 02:30:00 does not exist.
    assert_eq!(
        paris.to_utc(dt(2024, 3, 31, 2, 30, 0)),
        LocalToUtc::Nonexistent
    );
    // Local 2024-10-27 02:30:00 happens twice: 00:30:00 UTC with CEST, then
    // 01:30:00 UTC with CET.
    assert_eq!(
        paris.to_utc(dt(2024, 10, 27, 2, 30, 0)),
        LocalToUtc::Ambiguous {
            earlier: 1_729_989_000,
            later: 1_729_992_600
        }
    );
    // A normal local time maps to one instant: 2024-01-15 13:30:00 CET is
    // 12:30:00 UTC.
    assert_eq!(
        paris.to_utc(dt(2024, 1, 15, 13, 30, 0)),
        LocalToUtc::Unique(1_705_321_800)
    );
}

#[test]
fn america_new_york() {
    let new_york = TimeZone::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
    assert_eq!(new_york.std_offset_seconds(), -5 * 3600);
    assert_eq!(new_york.dst_offset_seconds(), Some(-4 * 3600));

    let local = new_york.from_utc(1_705_320_000); // 2024-01-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 1, 15, 7, 0, 0));
    assert_eq!(local.offset_seconds, -18_000);
    assert_eq!(local.abbreviation, "EST");
    assert!(!local.dst);

    let local = new_york.from_utc(1_721_044_800); // 2024-07-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 7, 15, 8, 0, 0));
    assert_eq!(local.offset_seconds, -14_400);
    assert_eq!(local.abbreviation, "EDT");
    assert!(local.dst);

    // Spring forward 2024 at 2024-03-10 07:00:00 UTC: 02:00:00 EST becomes
    // 03:00:00 EDT.
    let local = new_york.from_utc(1_710_053_999);
    assert_eq!(local.date_time, dt(2024, 3, 10, 1, 59, 59));
    assert_eq!(local.offset_seconds, -18_000);
    let local = new_york.from_utc(1_710_054_000);
    assert_eq!(local.date_time, dt(2024, 3, 10, 3, 0, 0));
    assert_eq!(local.offset_seconds, -14_400);

    // Autumn back 2024 at 2024-11-03 06:00:00 UTC: 02:00:00 EDT becomes
    // 01:00:00 EST.
    let local = new_york.from_utc(1_730_613_599);
    assert_eq!(local.date_time, dt(2024, 11, 3, 1, 59, 59));
    assert_eq!(local.offset_seconds, -14_400);
    let local = new_york.from_utc(1_730_613_600);
    assert_eq!(local.date_time, dt(2024, 11, 3, 1, 0, 0));
    assert_eq!(local.offset_seconds, -18_000);

    assert_eq!(
        new_york.to_utc(dt(2024, 3, 10, 2, 30, 0)),
        LocalToUtc::Nonexistent
    );
    assert_eq!(
        new_york.to_utc(dt(2024, 11, 3, 1, 30, 0)),
        LocalToUtc::Ambiguous {
            earlier: 1_730_611_800, // 05:30:00 UTC with EDT
            later: 1_730_615_400    // 06:30:00 UTC with EST
        }
    );
}

#[test]
fn australia_sydney_southern_hemisphere() {
    let sydney = TimeZone::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
    assert_eq!(sydney.std_offset_seconds(), 10 * 3600);
    assert_eq!(sydney.dst_offset_seconds(), Some(11 * 3600));

    // July is winter in Sydney.
    let local = sydney.from_utc(1_721_044_800); // 2024-07-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 7, 15, 22, 0, 0));
    assert_eq!(local.offset_seconds, 36_000);
    assert_eq!(local.abbreviation, "AEST");
    assert!(!local.dst);

    // January is summer.
    let local = sydney.from_utc(1_705_320_000); // 2024-01-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 1, 15, 23, 0, 0));
    assert_eq!(local.offset_seconds, 39_600);
    assert_eq!(local.abbreviation, "AEDT");
    assert!(local.dst);

    // Daylight saving starts on the first Sunday of October at 02:00:00 AEST,
    // which is 2024-10-05 16:00:00 UTC, and ends on the first Sunday of April
    // at 03:00:00 AEDT, which is 2024-04-06 16:00:00 UTC. The period crosses
    // New Year.
    let local = sydney.from_utc(1_728_143_999);
    assert_eq!(local.date_time, dt(2024, 10, 6, 1, 59, 59));
    assert_eq!(local.offset_seconds, 36_000);
    let local = sydney.from_utc(1_728_144_000);
    assert_eq!(local.date_time, dt(2024, 10, 6, 3, 0, 0));
    assert_eq!(local.offset_seconds, 39_600);

    let local = sydney.from_utc(1_712_419_199);
    assert_eq!(local.date_time, dt(2024, 4, 7, 2, 59, 59));
    assert_eq!(local.offset_seconds, 39_600);
    let local = sydney.from_utc(1_712_419_200);
    assert_eq!(local.date_time, dt(2024, 4, 7, 2, 0, 0));
    assert_eq!(local.offset_seconds, 36_000);

    // New Year is inside the daylight saving period.
    let local = sydney.from_utc(1_704_067_200); // 2024-01-01 00:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 1, 1, 11, 0, 0));
    assert!(local.dst);

    assert_eq!(
        sydney.to_utc(dt(2024, 10, 6, 2, 30, 0)),
        LocalToUtc::Nonexistent
    );
    assert_eq!(
        sydney.to_utc(dt(2024, 4, 7, 2, 30, 0)),
        LocalToUtc::Ambiguous {
            earlier: 1_712_417_400, // 2024-04-06 15:30:00 UTC with AEDT
            later: 1_712_421_000    // 2024-04-06 16:30:00 UTC with AEST
        }
    );
}

#[test]
fn asia_kolkata_half_hour_without_dst() {
    let kolkata = TimeZone::parse("IST-5:30").unwrap();
    assert!(!kolkata.has_dst());

    let local = kolkata.from_utc(1_717_200_000); // 2024-06-01 00:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 6, 1, 5, 30, 0));
    assert_eq!(local.offset_seconds, 19_800);
    assert_eq!(local.abbreviation, "IST");
    assert!(!local.dst);

    // No instant in a whole year has daylight saving time.
    let mut utc = 1_704_067_200; // 2024-01-01
    while utc < 1_735_689_600 {
        // 2025-01-01
        assert!(!kolkata.is_dst_at(utc));
        assert_eq!(kolkata.offset_at(utc), 19_800);
        utc += 3_600;
    }

    assert_eq!(
        kolkata.to_utc(dt(2024, 6, 1, 5, 30, 0)),
        LocalToUtc::Unique(1_717_200_000)
    );
}

#[test]
fn europe_dublin_negative_dst() {
    // The daylight saving offset of Dublin is smaller than the standard one:
    // the "DST" period is the winter GMT.
    let dublin = TimeZone::parse("IST-1GMT0,M10.5.0,M3.5.0/1").unwrap();
    assert_eq!(dublin.std_offset_seconds(), 3600);
    assert_eq!(dublin.dst_offset_seconds(), Some(0));

    // Summer: IST, no daylight saving time flag.
    let local = dublin.from_utc(1_721_044_800); // 2024-07-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 7, 15, 13, 0, 0));
    assert_eq!(local.offset_seconds, 3600);
    assert_eq!(local.abbreviation, "IST");
    assert!(!local.dst);

    // Winter: GMT, the daylight saving period.
    let local = dublin.from_utc(1_705_320_000); // 2024-01-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 1, 15, 12, 0, 0));
    assert_eq!(local.offset_seconds, 0);
    assert_eq!(local.abbreviation, "GMT");
    assert!(local.dst);

    // The GMT period starts on 2024-10-27 at 02:00:00 IST, which is
    // 01:00:00 UTC, and ends on 2024-03-31 at 01:00:00 GMT, which is
    // 01:00:00 UTC.
    let local = dublin.from_utc(1_729_990_799);
    assert_eq!(local.date_time, dt(2024, 10, 27, 1, 59, 59));
    assert_eq!(local.offset_seconds, 3600);
    assert_eq!(local.abbreviation, "IST");
    let local = dublin.from_utc(1_729_990_800);
    assert_eq!(local.date_time, dt(2024, 10, 27, 1, 0, 0));
    assert_eq!(local.offset_seconds, 0);
    assert_eq!(local.abbreviation, "GMT");
    assert!(local.dst);

    let local = dublin.from_utc(1_711_846_799);
    assert_eq!(local.date_time, dt(2024, 3, 31, 0, 59, 59));
    assert_eq!(local.offset_seconds, 0);
    assert!(local.dst);
    let local = dublin.from_utc(1_711_846_800);
    assert_eq!(local.date_time, dt(2024, 3, 31, 2, 0, 0));
    assert_eq!(local.offset_seconds, 3600);
    assert!(!local.dst);

    // Local 2024-10-27 01:30:00 happens twice: 00:30:00 UTC with IST, then
    // 01:30:00 UTC with GMT. Local 2024-03-31 01:30:00 does not exist.
    assert_eq!(
        dublin.to_utc(dt(2024, 10, 27, 1, 30, 0)),
        LocalToUtc::Ambiguous {
            earlier: 1_729_989_000,
            later: 1_729_992_600
        }
    );
    assert_eq!(
        dublin.to_utc(dt(2024, 3, 31, 1, 30, 0)),
        LocalToUtc::Nonexistent
    );
}

#[test]
fn half_hour_dst_shift_st_johns() {
    let st_johns = TimeZone::parse("NST3:30NDT,M3.2.0,M11.1.0").unwrap();
    assert_eq!(st_johns.std_offset_seconds(), -(3 * 3600 + 30 * 60));
    assert_eq!(st_johns.dst_offset_seconds(), Some(-(2 * 3600 + 30 * 60)));

    let local = st_johns.from_utc(1_721_044_800); // 2024-07-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 7, 15, 9, 30, 0));
    assert_eq!(local.offset_seconds, -9_000);
    assert_eq!(local.abbreviation, "NDT");
    assert!(local.dst);

    let local = st_johns.from_utc(1_705_320_000); // 2024-01-15 12:00:00 UTC
    assert_eq!(local.date_time, dt(2024, 1, 15, 8, 30, 0));
    assert_eq!(local.offset_seconds, -12_600);
    assert_eq!(local.abbreviation, "NST");
}

#[test]
fn rule_time_above_24_hours() {
    // A transition at 26:00 on the fourth Thursday of March is 02:00 on the
    // next day.
    let zone = TimeZone::parse("XXX-2YYY,M3.4.4/26,M10.5.0").unwrap();
    // The fourth Thursday of March 2024 is March 28, so the transition is
    // 2024-03-28 26:00:00 local standard time, that is 2024-03-29 02:00:00,
    // which is 2024-03-29 00:00:00 UTC.
    let start_utc = 1_711_670_400; // 2024-03-29 00:00:00 UTC
    let local = zone.from_utc(start_utc - 1);
    assert_eq!(local.date_time, dt(2024, 3, 29, 1, 59, 59));
    assert_eq!(local.offset_seconds, 7200);
    assert!(!local.dst);
    let local = zone.from_utc(start_utc);
    assert_eq!(local.date_time, dt(2024, 3, 29, 3, 0, 0));
    assert_eq!(local.offset_seconds, 10_800);
    assert!(local.dst);
    // Local 2024-03-29 02:30:00 does not exist.
    assert_eq!(
        zone.to_utc(dt(2024, 3, 29, 2, 30, 0)),
        LocalToUtc::Nonexistent
    );
}

#[test]
fn negative_rule_time() {
    // A transition at -1:00 is 23:00 on the previous day.
    let zone = TimeZone::parse("AAA2BBB,M3.5.0/-1,M10.5.0/0").unwrap();
    // The last Sunday of March 2024 is March 31, so the transition is
    // 2024-03-30 23:00:00 local standard time, which is 2024-03-31 01:00:00
    // UTC.
    let start_utc = 1_711_846_800; // 2024-03-31 01:00:00 UTC
    let local = zone.from_utc(start_utc - 1);
    assert_eq!(local.date_time, dt(2024, 3, 30, 22, 59, 59));
    assert_eq!(local.offset_seconds, -7200);
    assert!(!local.dst);
    let local = zone.from_utc(start_utc);
    assert_eq!(local.date_time, dt(2024, 3, 31, 0, 0, 0));
    assert_eq!(local.offset_seconds, -3600);
    assert!(local.dst);
}

#[test]
fn round_trip_over_transitions() {
    let zones = [
        TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap(),
        TimeZone::parse("EST5EDT,M3.2.0,M11.1.0").unwrap(),
        TimeZone::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap(),
        TimeZone::parse("IST-5:30").unwrap(),
        TimeZone::parse("IST-1GMT0,M10.5.0,M3.5.0/1").unwrap(),
    ];
    for zone in zones {
        // Every hour of 2024 and 2025, plus the seconds around each
        // transition, must map back to an instant that converts to the same
        // local time.
        let mut utc = 1_704_067_200; // 2024-01-01 00:00:00 UTC
        while utc < 1_767_225_600 {
            // 2026-01-01 00:00:00 UTC
            let local = zone.from_utc(utc);
            let back = zone.to_utc(local.date_time);
            match back {
                LocalToUtc::Unique(value) => assert_eq!(value, utc),
                LocalToUtc::Ambiguous { earlier, later } => {
                    assert!(earlier == utc || later == utc);
                }
                LocalToUtc::Nonexistent => panic!("utc {utc} maps to a nonexistent local time"),
            }
            utc += 3_600;
        }
    }
}

#[test]
fn local_to_utc_result_helpers() {
    let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
    let unique = paris.to_utc(dt(2024, 1, 15, 13, 30, 0));
    assert!(unique.is_unique());
    assert!(!unique.is_ambiguous());
    assert!(!unique.is_nonexistent());
    assert_eq!(unique.earliest(), Some(1_705_321_800));

    let ambiguous = paris.to_utc(dt(2024, 10, 27, 2, 30, 0));
    assert!(ambiguous.is_ambiguous());
    assert_eq!(ambiguous.earliest(), Some(1_729_989_000));

    let nonexistent = paris.to_utc(dt(2024, 3, 31, 2, 30, 0));
    assert!(nonexistent.is_nonexistent());
    assert_eq!(nonexistent.earliest(), None);
}
