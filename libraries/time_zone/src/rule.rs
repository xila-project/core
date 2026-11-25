//! The packed POSIX transition rule.
//!
//! A POSIX TZ string ends in two rules, for example `M3.5.0,M10.5.0/3`. Each
//! rule is packed into one 32-bit word, so `size_of::<Rule>() == 4` bytes.

use crate::civil::{SECONDS_PER_DAY, days_from_civil, days_in_month, weekday_from_days};
use core::fmt;

/// The kind of date that a [`Rule`] selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleKind {
    /// `Mm.w.d`: month m, week w (5 means the last), day of the week d
    /// (0 for Sunday).
    MonthWeekDay,
    /// `Jn`: day n of the year, 1 to 365, never counting February 29.
    JulianNoLeap,
    /// `n`: day n of the year, 0 to 365, counting February 29 in leap years.
    ZeroBasedDay,
}

/// The time of a rule without `/time`: 02:00:00.
pub(crate) const DEFAULT_RULE_TIME: i32 = 2 * 3600;

/// The largest rule time of POSIX 2024: 167:59:59.
pub(crate) const MAX_RULE_TIME: i32 = 167 * 3600 + 59 * 60 + 59;

/// Bias of the packed time field. A stored value of 0 means the default time
/// of a rule without `/time`; any other value is `time + TIME_BIAS`, which
/// keeps every valid time positive in the 21-bit field and leaves 0 unused.
const TIME_BIAS: i32 = MAX_RULE_TIME + 1;

/// One transition rule of a POSIX TZ string, for example `M3.5.0/2`.
///
/// The value is a `Copy` type of 32 bits (`size_of::<Rule>() == 4`):
///
/// - bits 0 to 1: the [`RuleKind`],
/// - bits 2 to 10: the date, 9 bits. For `Mm.w.d` it is
///   `((m - 1) * 5 + (w - 1)) * 7 + d`; for `Jn` it is `n - 1`; for `n` it is `n`,
/// - bits 11 to 31: the time of day, biased by `TIME_BIAS`; 0 means the
///   default 02:00:00 of a rule without `/time`. The bias covers the POSIX
///   2024 range of -167 to 167 hours with margin.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rule(u32);

const KIND_MASK: u32 = 0b11;
const DATE_MASK: u32 = 0b1_1111_1111; // 9 bits, shifted by 2

impl Rule {
    /// Pack a rule with an explicit time of day in seconds.
    pub(crate) const fn new(kind: RuleKind, date: u16, time_seconds: i32) -> Self {
        let kind = match kind {
            RuleKind::MonthWeekDay => 0,
            RuleKind::JulianNoLeap => 1,
            RuleKind::ZeroBasedDay => 2,
        };
        let time = (time_seconds + TIME_BIAS) as u32;
        Rule(kind | ((date as u32) << 2) | (time << 11))
    }

    /// Pack a rule whose time of day is the default 02:00:00.
    pub(crate) const fn implicit(kind: RuleKind, date: u16) -> Self {
        let kind = match kind {
            RuleKind::MonthWeekDay => 0,
            RuleKind::JulianNoLeap => 1,
            RuleKind::ZeroBasedDay => 2,
        };
        Rule(kind | ((date as u32) << 2))
    }

    /// The kind of date this rule selects.
    pub const fn kind(&self) -> RuleKind {
        match self.0 & KIND_MASK {
            0 => RuleKind::MonthWeekDay,
            1 => RuleKind::JulianNoLeap,
            _ => RuleKind::ZeroBasedDay,
        }
    }

    /// The 9-bit date code, see [`Rule`] for its layout.
    pub const fn date_code(&self) -> u16 {
        ((self.0 >> 2) & DATE_MASK) as u16
    }

    /// The packed time field: 0 for the default time, otherwise the time of
    /// day biased by [`TIME_BIAS`].
    const fn time_field(&self) -> u32 {
        self.0 >> 11
    }

    /// Whether the rule carries an explicit `/time`.
    pub const fn has_explicit_time(&self) -> bool {
        self.time_field() != 0
    }

    /// The time of day of the transition in seconds, signed. POSIX 2024 allows
    /// -167 to 167 hours. A rule without `/time` returns the default 02:00:00.
    pub const fn time_seconds(&self) -> i32 {
        let field = self.time_field();
        if field == 0 {
            DEFAULT_RULE_TIME
        } else {
            field as i32 - TIME_BIAS
        }
    }

    /// The month of an `Mm.w.d` rule, 1 to 12.
    pub const fn month(&self) -> u8 {
        (self.date_code() / 35 + 1) as u8
    }

    /// The week of an `Mm.w.d` rule, 1 to 5, where 5 means the last.
    pub const fn week(&self) -> u8 {
        (self.date_code() % 35 / 7 + 1) as u8
    }

    /// The day of the week of an `Mm.w.d` rule, 0 for Sunday.
    pub const fn weekday(&self) -> u8 {
        (self.date_code() % 7) as u8
    }

    /// The day of the year of a `Jn` rule, 1 to 365.
    pub const fn julian_day(&self) -> u16 {
        self.date_code() + 1
    }

    /// The day of the year of an `n` rule, 0 to 365.
    pub const fn zero_based_day(&self) -> u16 {
        self.date_code()
    }

    /// The day count from 1970-01-01 of the midnight that this rule selects in
    /// the given year.
    pub(crate) fn day_count(&self, year: i64) -> i64 {
        match self.kind() {
            RuleKind::MonthWeekDay => {
                let month = i64::from(self.month());
                let first = days_from_civil(year, month as u8, 1);
                let first_weekday = i64::from(weekday_from_days(first));
                let weekday = i64::from(self.weekday());
                let week = i64::from(self.week());
                let mut day_of_month = 1 + (weekday - first_weekday).rem_euclid(7) + (week - 1) * 7;
                let days_in_month = i64::from(days_in_month(year, month as u8));
                while day_of_month > days_in_month {
                    day_of_month -= 7;
                }
                days_from_civil(year, month as u8, day_of_month as u8)
            }
            RuleKind::JulianNoLeap => {
                // February 29 is never counted: days after it shift by one in
                // leap years.
                let day = i64::from(self.julian_day());
                let mut day_of_year = day - 1;
                if crate::civil::is_leap_year(year) && day >= 60 {
                    day_of_year += 1;
                }
                days_from_civil(year, 1, 1) + day_of_year
            }
            RuleKind::ZeroBasedDay => {
                days_from_civil(year, 1, 1) + i64::from(self.zero_based_day())
            }
        }
    }

    /// Seconds since the Unix epoch of the transition instant read in local
    /// rule time (the caller applies the offset that is in effect before the
    /// transition).
    pub(crate) fn local_seconds(&self, year: i64) -> i128 {
        // Computed in `i128` with multiply only (no division, which would
        // pull large helper routines into small targets) so that transition
        // instants near the ends of the `i64` second range stay exact.
        i128::from(self.day_count(year)) * i128::from(SECONDS_PER_DAY)
            + i128::from(self.time_seconds())
    }
}

impl fmt::Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            RuleKind::MonthWeekDay => {
                write!(f, "M{}.{}.{}", self.month(), self.week(), self.weekday())?
            }
            RuleKind::JulianNoLeap => write!(f, "J{}", self.julian_day())?,
            RuleKind::ZeroBasedDay => write!(f, "{}", self.zero_based_day())?,
        }
        if self.has_explicit_time() {
            write!(f, "/")?;
            let seconds = self.time_seconds();
            if seconds < 0 {
                write!(f, "-")?;
            }
            let absolute = seconds.abs();
            let hours = absolute / 3600;
            let minutes = (absolute % 3600) / 60;
            let rest = absolute % 60;
            if rest != 0 {
                write!(f, "{hours}:{minutes:02}:{rest:02}")?;
            } else if minutes != 0 {
                write!(f, "{hours}:{minutes:02}")?;
            } else {
                write!(f, "{hours}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_is_32_bits() {
        assert_eq!(core::mem::size_of::<Rule>(), 4);
        assert_eq!(core::mem::align_of::<Rule>(), 4);
    }

    #[test]
    fn pack_and_unpack_round_trip() {
        for (kind, date, time) in [
            (RuleKind::MonthWeekDay, 419, 7200),
            (RuleKind::MonthWeekDay, 0, -MAX_RULE_TIME),
            (RuleKind::MonthWeekDay, 209, MAX_RULE_TIME),
            (RuleKind::JulianNoLeap, 364, -1),
            (RuleKind::ZeroBasedDay, 365, 25 * 3600),
        ] {
            let rule = Rule::new(kind, date, time);
            assert_eq!(rule.kind(), kind);
            assert_eq!(rule.date_code(), date);
            assert_eq!(rule.time_seconds(), time);
            assert!(rule.has_explicit_time());
        }
    }

    #[test]
    fn implicit_time_is_the_default() {
        let rule = Rule::implicit(RuleKind::MonthWeekDay, 77);
        assert_eq!(rule.time_seconds(), DEFAULT_RULE_TIME);
        assert!(!rule.has_explicit_time());
        // An explicit /2 is kept apart from the default.
        let explicit = Rule::new(RuleKind::MonthWeekDay, 77, DEFAULT_RULE_TIME);
        assert!(explicit.has_explicit_time());
        assert_eq!(explicit.time_seconds(), DEFAULT_RULE_TIME);
    }

    #[test]
    fn debug_prints_the_posix_form() {
        let rule = Rule::new(
            RuleKind::MonthWeekDay,
            ((3 - 1) * 5 + (5 - 1)) * 7,
            3 * 3600,
        );
        assert_eq!(std::format!("{rule:?}"), "M3.5.0/3");
        let rule = Rule::new(
            RuleKind::MonthWeekDay,
            ((3 - 1) * 5 + (5 - 1)) * 7,
            2 * 3600,
        );
        assert_eq!(std::format!("{rule:?}"), "M3.5.0/2");
        let rule = Rule::implicit(RuleKind::MonthWeekDay, ((3 - 1) * 5 + (5 - 1)) * 7);
        assert_eq!(std::format!("{rule:?}"), "M3.5.0");
        let rule = Rule::new(
            RuleKind::MonthWeekDay,
            ((3 - 1) * 5 + (4 - 1)) * 7 + 4,
            2 * 3600 + 45 * 60,
        );
        assert_eq!(std::format!("{rule:?}"), "M3.4.4/2:45");
        let rule = Rule::new(
            RuleKind::MonthWeekDay,
            ((3 - 1) * 5 + (4 - 1)) * 7 + 4,
            26 * 3600,
        );
        assert_eq!(std::format!("{rule:?}"), "M3.4.4/26");
        let rule = Rule::new(RuleKind::MonthWeekDay, ((3 - 1) * 5 + (5 - 1)) * 7, -3600);
        assert_eq!(std::format!("{rule:?}"), "M3.5.0/-1");
        let rule = Rule::new(RuleKind::MonthWeekDay, ((3 - 1) * 5 + (5 - 1)) * 7, 0);
        assert_eq!(std::format!("{rule:?}"), "M3.5.0/0");
        let rule = Rule::new(RuleKind::JulianNoLeap, 364, 25 * 3600);
        assert_eq!(std::format!("{rule:?}"), "J365/25");
        let rule = Rule::new(RuleKind::ZeroBasedDay, 0, 0);
        assert_eq!(std::format!("{rule:?}"), "0/0");
    }

    #[test]
    fn month_week_day_dates() {
        // Last Sunday of March 2024 is March 31.
        let rule = Rule::new(RuleKind::MonthWeekDay, ((3 - 1) * 5 + (5 - 1)) * 7, 7200);
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((year, month, day), (2024, 3, 31));
        // First Sunday of October 2024 is October 6.
        let rule = Rule::new(RuleKind::MonthWeekDay, (10 - 1) * 5 * 7, 7200);
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((year, month, day), (2024, 10, 6));
        // Week 5 clamps to the last occurrence: last Saturday of September 2024
        // is September 28.
        let rule = Rule::new(RuleKind::MonthWeekDay, ((9 - 1) * 5 + (5 - 1)) * 7 + 6, 0);
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((year, month, day), (2024, 9, 28));
    }

    #[test]
    fn julian_rules_skip_february_29() {
        // J59 is February 28 in every year, because leap days are not counted.
        let rule = Rule::new(RuleKind::JulianNoLeap, 58, 0); // J59
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((year, month, day), (2024, 2, 28));
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2023));
        assert_eq!((year, month, day), (2023, 2, 28));
        // J60 is March 1 in every year.
        let rule = Rule::new(RuleKind::JulianNoLeap, 59, 0); // J60
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((year, month, day), (2024, 3, 1));
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2023));
        assert_eq!((year, month, day), (2023, 3, 1));
        // Zero based day 59 is February 29 in a leap year and March 1 otherwise.
        let rule = Rule::new(RuleKind::ZeroBasedDay, 59, 0);
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((year, month, day), (2024, 2, 29));
        let (year, month, day) = crate::civil::civil_from_days(rule.day_count(2023));
        assert_eq!((year, month, day), (2023, 3, 1));
    }
}
