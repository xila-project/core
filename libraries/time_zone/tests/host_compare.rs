//! Comparison of the conversions with the host IANA database through the
//! `chrono-tz` crate, which is a development dependency only.
//!
//! The TZ strings below are the POSIX footers of tzdata 2026f for the tested
//! zones. A POSIX footer holds only the current rules of a zone, so the
//! comparison starts in 2024, where the footers apply. UTC to local is
//! compared for every 6 hours from 2024 to 2038 and every 30 minutes from
//! 2024 to 2026; local to UTC is compared for local wall times on a 15 minute
//! grid over 2024 (which covers the spring gaps and autumn overlaps) and a
//! 30 minute grid over 2025 and 2026.

use chrono::{Datelike, LocalResult, NaiveDate, Offset as _, TimeZone as _, Timelike as _};
use chrono_tz::OffsetComponents as _;
use time_zone::{DateTime, LocalToUtc, TimeZone};

/// (IANA name, POSIX TZ string of tzdata 2026f).
const ZONES: &[(&str, &str)] = &[
    ("Europe/Paris", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("America/New_York", "EST5EDT,M3.2.0,M11.1.0"),
    ("Australia/Sydney", "AEST-10AEDT,M10.1.0,M4.1.0/3"),
    ("Asia/Kolkata", "IST-5:30"),
    ("Europe/Dublin", "IST-1GMT0,M10.5.0,M3.5.0/1"),
    (
        "Pacific/Chatham",
        "<+1245>-12:45<+1345>,M9.5.0/2:45,M4.1.0/3:45",
    ),
    ("America/St_Johns", "NST3:30NDT,M3.2.0,M11.1.0"),
    ("Asia/Tokyo", "JST-9"),
    ("Pacific/Apia", "<+13>-13"),
];

fn naive(date_time: &DateTime) -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(
        date_time.year as i32,
        u32::from(date_time.month),
        u32::from(date_time.day),
    )
    .unwrap()
    .and_hms_opt(
        u32::from(date_time.hour),
        u32::from(date_time.minute),
        u32::from(date_time.second),
    )
    .unwrap()
}

#[test]
fn utc_to_local_matches_the_host_database() {
    for (name, tz_string) in ZONES {
        let zone = TimeZone::parse(tz_string).unwrap();
        let host: chrono_tz::Tz = name.parse().unwrap();

        let mut utc = 1_704_067_200; // 2024-01-01 00:00:00 UTC
        while utc < 2_147_483_647 {
            // 2038-01-19 03:14:07 UTC
            check_utc_to_local(name, &zone, &host, utc);
            utc += 6 * 3600;
        }
        let mut utc = 1_704_067_200; // 2024-01-01 00:00:00 UTC
        while utc < 1_798_761_600 {
            // 2027-01-01 00:00:00 UTC
            check_utc_to_local(name, &zone, &host, utc);
            utc += 30 * 60;
        }
    }
}

fn check_utc_to_local(name: &str, zone: &TimeZone<'_>, host: &chrono_tz::Tz, utc: i64) {
    let local = zone.from_utc(utc);
    let host_local = host.timestamp_opt(utc, 0).single().unwrap();
    assert_eq!(
        local.date_time.year,
        i64::from(host_local.year()),
        "{name} {utc}"
    );
    assert_eq!(
        local.date_time.month,
        host_local.month() as u8,
        "{name} {utc}"
    );
    assert_eq!(local.date_time.day, host_local.day() as u8, "{name} {utc}");
    assert_eq!(
        local.date_time.hour,
        host_local.hour() as u8,
        "{name} {utc}"
    );
    assert_eq!(
        local.date_time.minute,
        host_local.minute() as u8,
        "{name} {utc}"
    );
    assert_eq!(
        local.date_time.second,
        host_local.second() as u8,
        "{name} {utc}"
    );
    assert_eq!(
        local.offset_seconds,
        host_local.offset().fix().local_minus_utc(),
        "{name} {utc}"
    );
    // `%Z` renders numeric abbreviations such as `+1345` as well.
    assert_eq!(
        local.abbreviation,
        host_local.format("%Z").to_string(),
        "{name} {utc}"
    );
    // The daylight saving flag is the host database's extra offset being
    // non-zero at that instant.
    let host_dst = host_local.offset().dst_offset().num_seconds() != 0;
    assert_eq!(local.dst, host_dst, "{name} {utc}");
}

#[test]
fn local_to_utc_matches_the_host_database() {
    for (name, tz_string) in ZONES {
        let zone = TimeZone::parse(tz_string).unwrap();
        let host: chrono_tz::Tz = name.parse().unwrap();

        for year in [2024, 2025, 2026] {
            let minutes: &[u8] = if year == 2024 {
                &[0, 15, 30, 45]
            } else {
                &[0, 30]
            };
            for day_of_year in 0..366 {
                let Some(date) = NaiveDate::from_ymd_opt(year, 1, 1)
                    .unwrap()
                    .checked_add_days(chrono::Days::new(day_of_year))
                else {
                    continue;
                };
                for hour in 0..24u8 {
                    for minute in minutes {
                        let date_time = DateTime {
                            year: i64::from(year),
                            month: date.month() as u8,
                            day: date.day() as u8,
                            hour,
                            minute: *minute,
                            second: 0,
                        };
                        let ours = zone.to_utc(date_time);
                        let host_result = host.from_local_datetime(&naive(&date_time));
                        match (ours, host_result) {
                            (LocalToUtc::Unique(utc), LocalResult::Single(dt)) => {
                                assert_eq!(utc, dt.timestamp(), "{name} {date_time:?}");
                            }
                            (
                                LocalToUtc::Ambiguous { earlier, later },
                                LocalResult::Ambiguous(earliest, latest),
                            ) => {
                                assert_eq!(earlier, earliest.timestamp(), "{name} {date_time:?}");
                                assert_eq!(later, latest.timestamp(), "{name} {date_time:?}");
                            }
                            (LocalToUtc::Nonexistent, LocalResult::None) => {}
                            (ours, host) => {
                                panic!("{name} {date_time:?}: ours {ours:?}, host {host:?}")
                            }
                        }
                    }
                }
            }
        }
    }
}
