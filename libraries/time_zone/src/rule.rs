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

/// One transition rule of a POSIX TZ string, for example `M3.5.0/2`.
///
/// The value is a `Copy` type of 32 bits (`size_of::<Rule>() == 4`):
///
/// - bits 0 to 1: the [`RuleKind`],
/// - bits 2 to 10: the date, 9 bits. For `Mm.w.d` it is
///   `((m - 1) * 5 + (w - 1)) * 7 + d`; for `Jn` it is `n - 1`; for `n` it is `n`,
/// - bits 11 to 31: the time of day in seconds as a signed 21-bit value, which
///   covers the POSIX 2024 range of -167 to 167 hours with margin.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rule(u32);

const KIND_MASK: u32 = 0b11;
const DATE_MASK: u32 = 0b1_1111_1111; // 9 bits, shifted by 2
const TIME_BITS: u32 = 21;

impl Rule {
    /// Pack a rule. `date` is the 9-bit date code, `time_seconds` the signed
    /// time of day in seconds.
    pub(crate) const fn new(kind: RuleKind, date: u16, time_seconds: i32) -> Self {
        let kind = match kind {
            RuleKind::MonthWeekDay => 0,
            RuleKind::JulianNoLeap => 1,
            RuleKind::ZeroBasedDay => 2,
        };
        let time = (time_seconds as u32) & ((1 << TIME_BITS) - 1);
        Rule(kind | ((date as u32) << 2) | (time << 11))
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

    /// The time of day of the transition in seconds, signed. POSIX 2024 allows
    /// -167 to 167 hours.
    pub const fn time_seconds(&self) -> i32 {
        // The time occupies the top 21 bits; an arithmetic shift right sign
        // extends it.
        (self.0 as i32) >> (32 - TIME_BITS)
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
                let n = i64::from(self.julian_day());
                let mut day_of_year = n - 1;
                if crate::civil::is_leap_year(year) && n >= 60 {
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
        self.day_count(year) as i128 * i128::from(SECONDS_PER_DAY) + i128::from(self.time_seconds())
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
        let time = self.time_seconds();
        if time != 2 * 3600 {
            write!(f, "/{}", time)?;
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
            (RuleKind::MonthWeekDay, 0, -601_259), // -167:59:59
            (RuleKind::MonthWeekDay, 209, 601_259), // 167:59:59
            (RuleKind::JulianNoLeap, 364, -1),
            (RuleKind::ZeroBasedDay, 365, 25 * 3600),
        ] {
            let rule = Rule::new(kind, date, time);
            assert_eq!(rule.kind(), kind);
            assert_eq!(rule.date_code(), date);
            assert_eq!(rule.time_seconds(), time);
        }
    }

    #[test]
    fn month_week_day_dates() {
        // Last Sunday of March 2024 is March 31.
        let rule = Rule::new(RuleKind::MonthWeekDay, ((3 - 1) * 5 + (5 - 1)) * 7, 7200);
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((y, m, d), (2024, 3, 31));
        // First Sunday of October 2024 is October 6.
        let rule = Rule::new(RuleKind::MonthWeekDay, (10 - 1) * 5 * 7, 7200);
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((y, m, d), (2024, 10, 6));
        // Week 5 clamps to the last occurrence: last Saturday of September 2024
        // is September 28.
        let rule = Rule::new(RuleKind::MonthWeekDay, ((9 - 1) * 5 + (5 - 1)) * 7 + 6, 0);
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((y, m, d), (2024, 9, 28));
    }

    #[test]
    fn julian_rules_skip_february_29() {
        // J59 is February 28 in every year, because leap days are not counted.
        let rule = Rule::new(RuleKind::JulianNoLeap, 58, 0); // J59
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((y, m, d), (2024, 2, 28));
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2023));
        assert_eq!((y, m, d), (2023, 2, 28));
        // J60 is March 1 in every year.
        let rule = Rule::new(RuleKind::JulianNoLeap, 59, 0); // J60
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((y, m, d), (2024, 3, 1));
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2023));
        assert_eq!((y, m, d), (2023, 3, 1));
        // Zero based day 59 is February 29 in a leap year and March 1 otherwise.
        let rule = Rule::new(RuleKind::ZeroBasedDay, 59, 0);
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2024));
        assert_eq!((y, m, d), (2024, 2, 29));
        let (y, m, d) = crate::civil::civil_from_days(rule.day_count(2023));
        assert_eq!((y, m, d), (2023, 3, 1));
    }
}
