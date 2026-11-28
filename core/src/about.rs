pub const fn get_major_version() -> u8 {
    match u8::from_str_radix(env!("CARGO_PKG_VERSION_MAJOR"), 10) {
        Ok(version) => version,
        Err(_) => panic!("Failed to parse major version"),
    }
}

pub const fn get_minor_version() -> u8 {
    match u8::from_str_radix(env!("CARGO_PKG_VERSION_MINOR"), 10) {
        Ok(version) => version,
        Err(_) => panic!("Failed to parse minor version"),
    }
}

pub const fn get_patch_version() -> u8 {
    match u8::from_str_radix(env!("CARGO_PKG_VERSION_PATCH"), 10) {
        Ok(version) => version,
        Err(_) => panic!("Failed to parse patch version"),
    }
}

pub const fn get_version_string() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub const fn get_authors() -> &'static str {
    env!("CARGO_PKG_AUTHORS")
}

pub const fn get_license() -> &'static str {
    env!("CARGO_PKG_LICENSE")
}

pub const fn get_description() -> &'static str {
    env!("CARGO_PKG_DESCRIPTION")
}

/// Short commit of the Platforms checkout that produced the final firmware,
/// exported as `XILA_BUILD_COMMIT` by the platform build (suffixed with
/// `-dirty` when the worktree has changes). `None` on a plain `cargo build`.
pub const fn get_commit() -> Option<&'static str> {
    option_env!("XILA_BUILD_COMMIT")
}

/// Build date of the final firmware as `YYYY-MM-DD` (UTC), from
/// `SOURCE_DATE_EPOCH`, which the platform build sets to the commit time.
/// `None` when the variable is missing or not a decimal number of seconds.
pub const fn get_build_date() -> Option<&'static str> {
    BUILD_DATE
}

const SECONDS_PER_DAY: u64 = 86_400;

const fn parse_epoch(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    let mut index = 0;
    while index < bytes.len() {
        let digit = match bytes[index] {
            byte @ b'0'..=b'9' => (byte - b'0') as u64,
            _ => return None,
        };
        value = match value.checked_mul(10) {
            Some(multiplied) => match multiplied.checked_add(digit) {
                Some(next) => next,
                None => return None,
            },
            None => return None,
        };
        index += 1;
    }
    Some(value)
}

/// Civil date from days since the Unix epoch (Howard Hinnant's algorithm).
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_pattern = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_pattern + 2) / 5 + 1;
    let month = if month_pattern < 10 {
        month_pattern + 3
    } else {
        month_pattern - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

const fn format_build_date(epoch: u64) -> Option<[u8; 10]> {
    let (year, month, day) = civil_from_days((epoch / SECONDS_PER_DAY) as i64);
    if year < 0 || year > 9_999 {
        return None;
    }
    let mut date = [b'0'; 10];
    date[0] = b'0' + (year / 1_000) as u8;
    date[1] = b'0' + ((year / 100) % 10) as u8;
    date[2] = b'0' + ((year / 10) % 10) as u8;
    date[3] = b'0' + (year % 10) as u8;
    date[4] = b'-';
    date[5] = b'0' + (month / 10) as u8;
    date[6] = b'0' + (month % 10) as u8;
    date[7] = b'-';
    date[8] = b'0' + (day / 10) as u8;
    date[9] = b'0' + (day % 10) as u8;
    Some(date)
}

const BUILD_DATE_BYTES: Option<[u8; 10]> = match option_env!("SOURCE_DATE_EPOCH") {
    Some(text) => match parse_epoch(text) {
        Some(epoch) => format_build_date(epoch),
        None => None,
    },
    None => None,
};

const BUILD_DATE: Option<&'static str> = match &BUILD_DATE_BYTES {
    // The formatted date holds only ASCII digits and hyphens.
    Some(bytes) => Some(unsafe { core::str::from_utf8_unchecked(bytes) }),
    None => None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_epochs() {
        assert_eq!(parse_epoch("0"), Some(0));
        assert_eq!(parse_epoch("1700000000"), Some(1_700_000_000));
        assert_eq!(parse_epoch(""), None);
        assert_eq!(parse_epoch("12a"), None);
        assert_eq!(parse_epoch("-1"), None);
        assert_eq!(parse_epoch("99999999999999999999999"), None);
    }

    #[test]
    fn formats_utc_dates() {
        assert_eq!(format_build_date(0), Some(*b"1970-01-01"));
        assert_eq!(format_build_date(86_399), Some(*b"1970-01-01"));
        assert_eq!(format_build_date(86_400), Some(*b"1970-01-02"));
        assert_eq!(format_build_date(1_700_000_000), Some(*b"2023-11-14"));
        assert_eq!(format_build_date(951_782_400), Some(*b"2000-02-29"));
        assert_eq!(format_build_date(4_102_444_800), Some(*b"2100-01-01"));
        assert_eq!(format_build_date(u64::MAX), None);
    }

    #[test]
    fn build_information_comes_from_the_compile_time_environment() {
        assert_eq!(get_commit(), option_env!("XILA_BUILD_COMMIT"));
        match option_env!("SOURCE_DATE_EPOCH")
            .and_then(parse_epoch)
            .and_then(format_build_date)
        {
            Some(bytes) => assert_eq!(
                get_build_date(),
                Some(core::str::from_utf8(&bytes).unwrap())
            ),
            None => assert_eq!(get_build_date(), None),
        }
    }
}
