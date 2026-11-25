//! The parsed time zone and the conversions between UTC and local civil time.

use crate::civil::{DateTime, SECONDS_PER_DAY, civil_from_days};
use crate::parse::{self, ParseError};
use crate::rule::Rule;
use core::fmt;

/// The number of bytes of an abbreviation buffer. The longest abbreviation in
/// tzdata 2026f is 5 bytes (`+0545`); POSIX requires at least 3 bytes.
pub const ABBREVIATION_MAX: usize = 7;

/// A zone abbreviation stored in a fixed-size ASCII buffer. The length is 0
/// when there is no daylight saving abbreviation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Abbreviation {
    bytes: [u8; ABBREVIATION_MAX],
    length: u8,
}

impl Abbreviation {
    pub(crate) const EMPTY: Self = Self {
        bytes: [0; ABBREVIATION_MAX],
        length: 0,
    };

    /// Store a name that the parser already validated as ASCII. Returns None
    /// when the name does not fit the buffer.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        if name.len() > ABBREVIATION_MAX || !name.is_ascii() {
            return None;
        }
        let mut bytes = [0; ABBREVIATION_MAX];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Some(Self {
            bytes,
            length: name.len() as u8,
        })
    }

    fn as_str(&self) -> &str {
        // The parser only accepts ASCII names.
        core::str::from_utf8(&self.bytes[..self.length as usize]).unwrap_or("")
    }
}

impl fmt::Debug for Abbreviation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Marks a time zone without daylight saving time in
/// [`TimeZone::daylight_saving_offset`]. Real offsets are at most 24 hours, so
/// this value never collides with one.
const NO_DAYLIGHT_SAVING: i32 = i32::MIN;

/// A parsed POSIX TZ string.
///
/// The value is `Copy`, owns its two abbreviations in fixed-size buffers and
/// stores the two 32-bit [`Rule`] values and the two offsets inline. It is 32
/// bytes on every target (`size_of::<TimeZone>() == 32`) and performs no
/// allocation, so a TZ string read from settings can be parsed once and the
/// value kept by value.
///
/// Offsets are seconds east of UTC, so `Europe/Paris` has a standard offset of
/// 3600 and a daylight saving offset of 7200.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeZone {
    standard_abbreviation: Abbreviation,
    daylight_saving_abbreviation: Abbreviation,
    standard_offset: i32,
    /// Seconds east of UTC during daylight saving time, or
    /// [`NO_DAYLIGHT_SAVING`] when the zone has none.
    daylight_saving_offset: i32,
    start: Rule,
    end: Rule,
}

impl TimeZone {
    /// Parse a POSIX TZ string, for example `CET-1CEST,M3.5.0,M10.5.0/3` for
    /// `Europe/Paris`.
    ///
    /// The format is the `TZ` format of POSIX `tzset`:
    /// `std offset [dst [offset] [,rule,rule]]`.
    ///
    /// - A name is either at least three letters (`CET`) or quoted in angle
    ///   brackets with at least three ASCII alphanumeric characters, `+` or
    ///   `-` (`<+03>`). A name longer than 7 bytes is rejected with
    ///   [`ParseError::NameTooLong`].
    /// - An offset is `[+-]hh[:mm[:ss]]` of at most 24 hours; minutes and
    ///   seconds are 0 to 59. A bare name without an offset, for example
    ///   `UTC`, is rejected with [`ParseError::MissingOffset`].
    /// - A rule is `Mm.w.d[/time]`, `Jn[/time]` or `n[/time]`. A rule time is a
    ///   signed `[+-]hh[:mm[:ss]]` of at most 167 hours (POSIX 2024), with
    ///   minutes and seconds of 0 to 59, and it defaults to 02:00:00.
    /// - A daylight saving name without rules uses the United States rules
    ///   `M3.2.0/2,M11.1.0/2`, and a daylight saving offset without a value is
    ///   one hour ahead of the standard offset.
    ///
    /// Invalid strings return a [`ParseError`] that says which part is wrong.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        parse::parse(input)
    }

    pub(crate) const fn fixed(standard_abbreviation: Abbreviation, standard_offset: i32) -> Self {
        Self {
            standard_abbreviation,
            daylight_saving_abbreviation: Abbreviation::EMPTY,
            standard_offset,
            daylight_saving_offset: NO_DAYLIGHT_SAVING,
            start: Rule::implicit(crate::rule::RuleKind::MonthWeekDay, 0),
            end: Rule::implicit(crate::rule::RuleKind::MonthWeekDay, 0),
        }
    }

    pub(crate) const fn with_daylight_saving(
        standard_abbreviation: Abbreviation,
        standard_offset: i32,
        daylight_saving_abbreviation: Abbreviation,
        daylight_saving_offset: i32,
        start: Rule,
        end: Rule,
    ) -> Self {
        Self {
            standard_abbreviation,
            daylight_saving_abbreviation,
            standard_offset,
            daylight_saving_offset,
            start,
            end,
        }
    }

    /// The abbreviation of the standard offset, for example `CET`.
    pub fn standard_abbreviation(&self) -> &str {
        self.standard_abbreviation.as_str()
    }

    /// The abbreviation of the daylight saving offset, if the zone has one.
    pub fn daylight_saving_abbreviation(&self) -> Option<&str> {
        if self.has_daylight_saving() {
            Some(self.daylight_saving_abbreviation.as_str())
        } else {
            None
        }
    }

    /// The standard offset in seconds east of UTC.
    pub fn standard_offset_seconds(&self) -> i32 {
        self.standard_offset
    }

    /// The daylight saving offset in seconds east of UTC, if the zone has one.
    pub fn daylight_saving_offset_seconds(&self) -> Option<i32> {
        if self.has_daylight_saving() {
            Some(self.daylight_saving_offset)
        } else {
            None
        }
    }

    /// Whether the zone has a daylight saving period.
    pub fn has_daylight_saving(&self) -> bool {
        self.daylight_saving_offset != NO_DAYLIGHT_SAVING
    }

    /// The rule that starts the daylight saving period.
    pub fn start_rule(&self) -> Rule {
        self.start
    }

    /// The rule that ends the daylight saving period.
    pub fn end_rule(&self) -> Rule {
        self.end
    }

    /// Whether daylight saving time applies at a UTC instant.
    pub fn is_daylight_saving_at(&self, utc_seconds: i64) -> bool {
        if !self.has_daylight_saving() {
            return false;
        }
        // The rules are written in local time; pick the years around the
        // instant so that periods that cross New Year are covered. The day
        // split keeps the addition of the offset inside the `i64` range for
        // every `i64` instant.
        let day_of_utc = utc_seconds.div_euclid(SECONDS_PER_DAY);
        let second_of_day = utc_seconds.rem_euclid(SECONDS_PER_DAY);
        let local_days = day_of_utc
            + (second_of_day + i64::from(self.standard_offset)).div_euclid(SECONDS_PER_DAY);
        let year = civil_from_days(local_days).0;
        let utc = i128::from(utc_seconds);
        for year in [year - 1, year, year + 1] {
            // The start time is read in standard time, the end time in
            // daylight saving time, as POSIX tzset does.
            let start = self.start.local_seconds(year) - i128::from(self.standard_offset);
            let end = self.end.local_seconds(year) - i128::from(self.daylight_saving_offset);
            if start < end {
                if start <= utc && utc < end {
                    return true;
                }
            } else if start > end {
                // The daylight saving period crosses New Year, so it ends with
                // the end rule of the next year.
                let end_next =
                    self.end.local_seconds(year + 1) - i128::from(self.daylight_saving_offset);
                if utc >= start && utc < end_next {
                    return true;
                }
            }
        }
        false
    }

    /// The offset in seconds east of UTC at a UTC instant.
    pub fn offset_at(&self, utc_seconds: i64) -> i32 {
        if self.has_daylight_saving() && self.is_daylight_saving_at(utc_seconds) {
            self.daylight_saving_offset
        } else {
            self.standard_offset
        }
    }

    /// Convert a UTC instant, in seconds since the Unix epoch, to local time.
    ///
    /// The result holds the local date and time, the offset in seconds east of
    /// UTC, the zone abbreviation and whether daylight saving time applies.
    /// The conversion is total for every `i64` second count.
    pub fn from_utc(&self, utc_seconds: i64) -> Local<'_> {
        let daylight_saving = self.is_daylight_saving_at(utc_seconds);
        let offset = if daylight_saving {
            self.daylight_saving_offset
        } else {
            self.standard_offset
        };
        // Split the instant into day and second of day first so that adding
        // the offset (at most 24 hours) cannot leave the `i64` range.
        let day_of_utc = utc_seconds.div_euclid(SECONDS_PER_DAY);
        let second_of_day_utc = utc_seconds.rem_euclid(SECONDS_PER_DAY);
        let local_second_of_day = second_of_day_utc + i64::from(offset);
        let days = day_of_utc + local_second_of_day.div_euclid(SECONDS_PER_DAY);
        let second_of_day = local_second_of_day.rem_euclid(SECONDS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        let abbreviation = if daylight_saving {
            self.daylight_saving_abbreviation.as_str()
        } else {
            self.standard_abbreviation.as_str()
        };
        Local {
            date_time: DateTime {
                year,
                month,
                day,
                hour: (second_of_day / 3600) as u8,
                minute: ((second_of_day % 3600) / 60) as u8,
                second: (second_of_day % 60) as u8,
            },
            offset_seconds: offset,
            abbreviation,
            daylight_saving,
        }
    }

    /// Convert a local date and time to UTC, in seconds since the Unix epoch.
    ///
    /// The result is explicit about repeated and missing local times:
    /// [`LocalToUtc::Ambiguous`] for a local time that happens twice (the
    /// repeated hour in autumn) and [`LocalToUtc::Nonexistent`] for a local
    /// time that never happens (the skipped hour in spring).
    ///
    /// The input fields are not checked; see [`DateTime::is_valid`]. A local
    /// time whose instants all fall outside the range that `i64` seconds can
    /// hold also returns [`LocalToUtc::Nonexistent`], because no UTC instant
    /// can represent it.
    pub fn to_utc(&self, local: DateTime) -> LocalToUtc {
        // A local time whose seconds count leaves the `i64` range has no
        // representable UTC instant.
        let Some(local_seconds) = local.unix_seconds_checked() else {
            return LocalToUtc::Nonexistent;
        };
        let mut found = [0i64; 2];
        let mut count = 0;
        // Both offsets that the local time could use; they are equal when the
        // zone has no daylight saving time, and the duplicate candidate below
        // is removed.
        let offsets = [
            self.standard_offset,
            if self.has_daylight_saving() {
                self.daylight_saving_offset
            } else {
                self.standard_offset
            },
        ];
        for offset in offsets {
            // A candidate outside the i64 range cannot be represented.
            let Some(utc) = local_seconds.checked_sub(i64::from(offset)) else {
                continue;
            };
            // The candidate is valid when the zone really uses this offset at
            // that instant.
            if self.offset_at(utc) == offset && !found[..count].contains(&utc) {
                found[count] = utc;
                count += 1;
            }
        }
        match count {
            0 => LocalToUtc::Nonexistent,
            1 => LocalToUtc::Unique(found[0]),
            _ => LocalToUtc::Ambiguous {
                earlier: found[0].min(found[1]),
                later: found[0].max(found[1]),
            },
        }
    }
}

/// The result of converting a UTC instant to local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local<'a> {
    /// The local date and time.
    pub date_time: DateTime,
    /// The offset in seconds east of UTC at that instant.
    pub offset_seconds: i32,
    /// The zone abbreviation at that instant, for example `CEST`.
    pub abbreviation: &'a str,
    /// Whether daylight saving time applies.
    pub daylight_saving: bool,
}

/// The result of converting a local date and time to UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalToUtc {
    /// The local time maps to exactly one UTC instant, in seconds since the
    /// Unix epoch.
    Unique(i64),
    /// The local time happens twice, because the clocks were set back. `earlier`
    /// and `later` are the two UTC instants in seconds since the Unix epoch;
    /// `earlier` is the one with the larger offset east of UTC.
    Ambiguous {
        /// The first of the two instants.
        earlier: i64,
        /// The second of the two instants.
        later: i64,
    },
    /// The local time never happens, because the clocks were set forward, or
    /// because no UTC instant in the `i64` second range can represent it.
    Nonexistent,
}

impl LocalToUtc {
    /// Whether the local time maps to exactly one instant.
    pub fn is_unique(&self) -> bool {
        matches!(self, LocalToUtc::Unique(_))
    }

    /// Whether the local time happens twice.
    pub fn is_ambiguous(&self) -> bool {
        matches!(self, LocalToUtc::Ambiguous { .. })
    }

    /// Whether the local time never happens.
    pub fn is_nonexistent(&self) -> bool {
        matches!(self, LocalToUtc::Nonexistent)
    }

    /// The earliest UTC instant that matches the local time, in seconds since
    /// the Unix epoch. None when the local time never happens.
    pub fn earliest(&self) -> Option<i64> {
        match self {
            LocalToUtc::Unique(utc) => Some(*utc),
            LocalToUtc::Ambiguous { earlier, .. } => Some(*earlier),
            LocalToUtc::Nonexistent => None,
        }
    }
}
