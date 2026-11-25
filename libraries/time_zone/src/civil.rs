//! Proleptic Gregorian calendar arithmetic without allocation.
//!
//! Days are counted from 1970-01-01, the Unix epoch. The formulas are the
//! civil-from-days and days-from-civil algorithms published by Howard Hinnant
//! (<https://howardhinnant.github.io/date_algorithms.html>); the variable names
//! `mp`, `doe`, `yoe` and `doy` below are the ones of that published algorithm.

/// Seconds in one day.
pub(crate) const SECONDS_PER_DAY: i64 = 86_400;

/// A broken down date and time in the proleptic Gregorian calendar.
///
/// The fields must hold the values of their names (month 1 to 12, and so on);
/// see [`DateTime::is_valid`]. Conversions do not check them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    /// The year, for example 2026.
    pub year: i64,
    /// The month of the year, 1 to 12.
    pub month: u8,
    /// The day of the month, 1 to 31.
    pub day: u8,
    /// The hour of the day, 0 to 23.
    pub hour: u8,
    /// The minute of the hour, 0 to 59.
    pub minute: u8,
    /// The second of the minute, 0 to 59. Leap seconds are not represented.
    pub second: u8,
}

impl DateTime {
    /// Whether every field holds a value of its name: month 1 to 12, day 1 to
    /// the length of the month, hour 0 to 23, minute and second 0 to 59.
    pub const fn is_valid(&self) -> bool {
        self.month >= 1
            && self.month <= 12
            && self.day >= 1
            && self.day <= days_in_month(self.year, self.month)
            && self.hour <= 23
            && self.minute <= 59
            && self.second <= 59
    }

    /// Seconds since the Unix epoch of this date and time read as UTC, or
    /// `None` when the fields are so far out that the count leaves the `i64`
    /// range. The fields themselves are not checked; see [`DateTime::is_valid`].
    pub(crate) fn unix_seconds_checked(self) -> Option<i64> {
        let days = days_from_civil_checked(self.year, self.month, self.day)?;
        let second_of_day =
            i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second);
        days.checked_mul(SECONDS_PER_DAY)?
            .checked_add(second_of_day)
    }
}

/// Days from 1970-01-01 to the given civil date, or `None` when the year is
/// so far out that the count leaves the `i64` range. Used only for unchecked
/// local input of [`crate::TimeZone::to_utc`]; the hot paths use
/// [`days_from_civil`] and [`civil_from_days`], whose results always stay in
/// range.
pub(crate) fn days_from_civil_checked(year: i64, month: u8, day: u8) -> Option<i64> {
    let year = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400); // [0, 399]
    let mp = (month as i64 + 9) % 12; // March is 0
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe
        .checked_mul(365)?
        .checked_add(yoe / 4)?
        .checked_sub(yoe / 100)?
        .checked_add(doy)?; // [0, 146096]
    era.checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)
}

/// Days from 1970-01-01 to the given civil date. The inverse of
/// [`civil_from_days`]. Every `i64` second count maps to a day count that
/// stays inside the `i64` range, so `i64` arithmetic is exact here and keeps
/// the code small on targets without a hardware divide.
pub const fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400; // [0, 399]
    let mp = (month as i64 + 9) % 12; // March is 0
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count from 1970-01-01, as (year, month, day).
/// The inverse of [`days_from_civil`].
pub const fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// The day of the week of a day count from 1970-01-01, 0 for Sunday.
pub(crate) const fn weekday_from_days(days: i64) -> u8 {
    ((days + 4).rem_euclid(7)) as u8
}

/// Whether a year is a leap year in the Gregorian calendar.
pub(crate) const fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The number of days in a month, or 0 for a month outside 1 to 12.
pub(crate) const fn days_in_month(year: i64, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(weekday_from_days(0), 4); // Thursday
    }

    #[test]
    fn round_trip_over_a_wide_range() {
        let mut days = -800_000;
        while days < 800_000 {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
            days += 977;
        }
    }

    #[test]
    fn leap_rules() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(
            civil_from_days(days_from_civil(2024, 3, 1) - 1),
            (2024, 2, 29)
        );
    }

    #[test]
    fn date_time_validity() {
        assert!(
            DateTime {
                year: 2024,
                month: 2,
                day: 29,
                hour: 23,
                minute: 59,
                second: 59
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2023,
                month: 2,
                day: 29,
                hour: 0,
                minute: 0,
                second: 0
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 13,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 0,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 4,
                day: 31,
                hour: 0,
                minute: 0,
                second: 0
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 1,
                day: 1,
                hour: 24,
                minute: 0,
                second: 0
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 1,
                day: 1,
                hour: 0,
                minute: 60,
                second: 0
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 60
            }
            .is_valid()
        );
        assert!(
            !DateTime {
                year: 2024,
                month: 1,
                day: 0,
                hour: 0,
                minute: 0,
                second: 0
            }
            .is_valid()
        );
    }
}
