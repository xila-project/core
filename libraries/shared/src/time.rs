pub fn decompose_unix_timestamp(unix_timestamp: i64) -> (u16, u8, u8, u8, u8, u8) {
    // Constants for calculations
    const SECONDS_IN_MINUTE: i64 = 60;
    const SECONDS_IN_HOUR: i64 = 60 * SECONDS_IN_MINUTE;
    const SECONDS_IN_DAY: i64 = 24 * SECONDS_IN_HOUR;
    const DAYS_IN_YEAR: i64 = 365;
    const DAYS_IN_LEAP_YEAR: i64 = 366;

    // Start from 1970.
    let mut year: i64 = 1970;
    let mut days_since_epoch = unix_timestamp.div_euclid(SECONDS_IN_DAY);
    let mut remaining_seconds = unix_timestamp.rem_euclid(SECONDS_IN_DAY);

    // Determine the current year for timestamps on/after and before epoch.
    while days_since_epoch
        >= if is_leap_year(year) {
            DAYS_IN_LEAP_YEAR
        } else {
            DAYS_IN_YEAR
        }
    {
        days_since_epoch -= if is_leap_year(year) {
            DAYS_IN_LEAP_YEAR
        } else {
            DAYS_IN_YEAR
        };
        year += 1;
    }

    while days_since_epoch < 0 {
        year -= 1;
        days_since_epoch += if is_leap_year(year) {
            DAYS_IN_LEAP_YEAR
        } else {
            DAYS_IN_YEAR
        };
    }

    // Determine the current month and day
    let mut month = 0;
    while days_since_epoch >= days_in_month(year, month) {
        days_since_epoch -= days_in_month(year, month);
        month += 1;
    }

    // Remaining days are the day of the month
    let day = days_since_epoch + 1;

    // Calculate hour, minute, and second from remaining seconds
    let hour = remaining_seconds / SECONDS_IN_HOUR;
    remaining_seconds %= SECONDS_IN_HOUR;
    let minute = remaining_seconds / SECONDS_IN_MINUTE;
    let second = remaining_seconds % SECONDS_IN_MINUTE;

    (
        year as u16,
        month as u8 + 1,
        day as u8,
        hour as u8,
        minute as u8,
        second as u8,
    )
}

pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub fn days_in_month(year: i64, month: usize) -> i64 {
    // Number of days in each month (non-leap year)
    const DAYS_IN_MONTH: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    if month == 1 && is_leap_year(year) {
        // February in a leap year
        29
    } else {
        DAYS_IN_MONTH[month]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_is_correct() {
        assert_eq!(decompose_unix_timestamp(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn one_second_before_epoch_is_correct() {
        assert_eq!(decompose_unix_timestamp(-1), (1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn one_day_before_epoch_is_correct() {
        assert_eq!(decompose_unix_timestamp(-86_400), (1969, 12, 31, 0, 0, 0));
    }

    #[test]
    fn leap_day_2024_is_correct() {
        // 2024-02-29 00:00:00 UTC
        assert_eq!(
            decompose_unix_timestamp(1_709_164_800),
            (2024, 2, 29, 0, 0, 0)
        );
    }

    #[test]
    fn leap_year_rules_are_correct() {
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
    }

    #[test]
    fn february_days_are_correct() {
        assert_eq!(days_in_month(2024, 1), 29);
        assert_eq!(days_in_month(2023, 1), 28);
    }
}
