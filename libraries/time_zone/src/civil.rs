//! Proleptic Gregorian calendar arithmetic without allocation.
//!
//! Days are counted from 1970-01-01, the Unix epoch. The formulas are the
//! civil-from-days and days-from-civil algorithms by Howard Hinnant; they stay
//! exact for every date that an `i64` second count can reach.

/// Seconds in one day.
pub(crate) const SECONDS_PER_DAY: i64 = 86_400;

/// Largest day count that still fits in `i64` seconds.
pub(crate) const MAX_DAYS: i64 = i64::MAX.div_euclid(SECONDS_PER_DAY);

/// Smallest day count that still fits in `i64` seconds.
pub(crate) const MIN_DAYS: i64 = i64::MIN.div_euclid(SECONDS_PER_DAY);

/// A broken down date and time in the proleptic Gregorian calendar.
///
/// The fields must hold the values of their names (month 1 to 12, and so on);
/// conversions do not check them.
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
    /// Seconds since the Unix epoch of this date and time read as UTC.
    ///
    /// The count is computed in `i128`; years outside the range that `i64`
    /// seconds can hold saturate at the `i64` bounds.
    pub(crate) fn unix_seconds_wide(self) -> i128 {
        let year = self
            .year
            .clamp(civil_from_days(MIN_DAYS).0, civil_from_days(MAX_DAYS).0);
        let days = days_from_civil(year, self.month, self.day) as i128;
        let second_of_day =
            i128::from(self.hour) * 3600 + i128::from(self.minute) * 60 + i128::from(self.second);
        days * i128::from(SECONDS_PER_DAY) + second_of_day
    }
}

/// Days from 1970-01-01 to the given civil date. The inverse of
/// [`civil_from_days`].
pub const fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let mp = (month as i64 + 9) % 12; // March is 0
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count from 1970-01-01, as (year, month, day).
/// The inverse of [`days_from_civil`].
pub const fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8; // [1, 12]
    (if month <= 2 { y + 1 } else { y }, month, day)
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
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
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
}
