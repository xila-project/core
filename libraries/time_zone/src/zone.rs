//! The parsed time zone and the conversions between UTC and local civil time.

use crate::civil::{DateTime, SECONDS_PER_DAY, civil_from_days};
use crate::parse::{self, ParseError};
use crate::rule::Rule;

/// A parsed POSIX TZ string.
///
/// The value is `Copy`; it borrows the two zone abbreviations from the input
/// string and stores everything else inline: two 32-bit [`Rule`] values and two
/// offsets. Parsing performs no allocation.
///
/// Offsets are seconds east of UTC, so `Europe/Paris` has a standard offset of
/// 3600 and a daylight saving offset of 7200.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeZone<'a> {
    std_abbr: &'a str,
    dst_abbr: Option<&'a str>,
    std_offset: i32,
    dst_offset: Option<i32>,
    start: Rule,
    end: Rule,
}

impl<'a> TimeZone<'a> {
    /// Parse a POSIX TZ string, for example `CET-1CEST,M3.5.0,M10.5.0/3`.
    ///
    /// The returned value borrows its abbreviations from `input`.
    pub fn parse(input: &'a str) -> Result<Self, ParseError> {
        parse::parse(input)
    }

    pub(crate) const fn fixed(std_abbr: &'a str, std_offset: i32) -> Self {
        Self {
            std_abbr,
            dst_abbr: None,
            std_offset,
            dst_offset: None,
            start: Rule::new(crate::rule::RuleKind::MonthWeekDay, 0, 0),
            end: Rule::new(crate::rule::RuleKind::MonthWeekDay, 0, 0),
        }
    }

    pub(crate) const fn with_dst(
        std_abbr: &'a str,
        std_offset: i32,
        dst_abbr: &'a str,
        dst_offset: i32,
        start: Rule,
        end: Rule,
    ) -> Self {
        Self {
            std_abbr,
            dst_abbr: Some(dst_abbr),
            std_offset,
            dst_offset: Some(dst_offset),
            start,
            end,
        }
    }

    /// The abbreviation of the standard offset, for example `CET`.
    pub fn std_abbreviation(&self) -> &'a str {
        self.std_abbr
    }

    /// The abbreviation of the daylight saving offset, if the zone has one.
    pub fn dst_abbreviation(&self) -> Option<&'a str> {
        self.dst_abbr
    }

    /// The standard offset in seconds east of UTC.
    pub fn std_offset_seconds(&self) -> i32 {
        self.std_offset
    }

    /// The daylight saving offset in seconds east of UTC, if the zone has one.
    pub fn dst_offset_seconds(&self) -> Option<i32> {
        self.dst_offset
    }

    /// Whether the zone has a daylight saving period.
    pub fn has_dst(&self) -> bool {
        self.dst_offset.is_some()
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
    pub fn is_dst_at(&self, utc_seconds: i64) -> bool {
        let Some(dst_offset) = self.dst_offset else {
            return false;
        };
        // The rules are written in local time; pick the years around the
        // instant so that periods that cross New Year are covered.
        let local_days = (utc_seconds as i128 + i128::from(self.std_offset))
            .div_euclid(i128::from(SECONDS_PER_DAY));
        let year = civil_from_days(local_days as i64).0;
        let utc = utc_seconds as i128;
        for y in [year - 1, year, year + 1] {
            // The start time is read in standard time, the end time in
            // daylight saving time, as POSIX tzset does.
            let start = self.start.local_seconds(y) - i128::from(self.std_offset);
            let end = self.end.local_seconds(y) - i128::from(dst_offset);
            if start < end {
                if start <= utc && utc < end {
                    return true;
                }
            } else if start > end {
                // The daylight saving period crosses New Year.
                let end_next = self.end.local_seconds(y + 1) - i128::from(dst_offset);
                if utc >= start && utc < end_next {
                    return true;
                }
            }
        }
        false
    }

    /// The offset in seconds east of UTC at a UTC instant.
    pub fn offset_at(&self, utc_seconds: i64) -> i32 {
        match self.dst_offset {
            Some(dst_offset) if self.is_dst_at(utc_seconds) => dst_offset,
            _ => self.std_offset,
        }
    }

    /// Convert a UTC instant, in seconds since the Unix epoch, to local time.
    ///
    /// The result holds the local date and time, the offset in seconds east of
    /// UTC, the zone abbreviation and whether daylight saving time applies.
    pub fn from_utc(&self, utc_seconds: i64) -> Local<'a> {
        let dst = self.has_dst() && self.is_dst_at(utc_seconds);
        let offset = if dst {
            self.dst_offset.unwrap_or(self.std_offset)
        } else {
            self.std_offset
        };
        let local = utc_seconds as i128 + i128::from(offset);
        let days = local.div_euclid(i128::from(SECONDS_PER_DAY));
        let second_of_day = local.rem_euclid(i128::from(SECONDS_PER_DAY));
        let (year, month, day) = civil_from_days(days as i64);
        let abbreviation = match (self.dst_abbr, dst) {
            (Some(abbr), true) => abbr,
            _ => self.std_abbr,
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
            dst,
        }
    }

    /// Convert a local date and time to UTC, in seconds since the Unix epoch.
    ///
    /// The result is explicit about repeated and missing local times:
    /// [`LocalToUtc::Ambiguous`] for a local time that happens twice (the
    /// repeated hour in autumn) and [`LocalToUtc::Nonexistent`] for a local
    /// time that never happens (the skipped hour in spring).
    pub fn to_utc(&self, local: DateTime) -> LocalToUtc {
        let local_seconds = local.unix_seconds_wide();
        let mut found = [0i64; 2];
        let mut count = 0;
        // Both offsets that the local time could use; they are equal when the
        // zone has no daylight saving time, and the duplicate candidate below
        // is removed.
        let offsets = [self.std_offset, self.dst_offset.unwrap_or(self.std_offset)];
        for offset in offsets {
            let utc = local_seconds - i128::from(offset);
            let utc = utc.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64;
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
    pub dst: bool,
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
    /// The local time never happens, because the clocks were set forward.
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
