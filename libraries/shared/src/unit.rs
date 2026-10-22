use core::fmt::{self, Debug, Display};

pub struct Suffix {
    pub name: &'static str,
    pub symbol: &'static str,
}

pub type PrefixTuple = (i32, &'static str, &'static str);

pub const BYTES_SUFFIX: Suffix = Suffix {
    name: "bytes",
    symbol: "B",
};
pub const BITS_SUFFIX: Suffix = Suffix {
    name: "bits",
    symbol: "b",
};
pub const FREQUENCY_SUFFIX: Suffix = Suffix {
    name: "hertz",
    symbol: "Hz",
};

pub const PREFIXES: &[PrefixTuple] = &[
    (30, "quetta", "Q"),
    (27, "ronna", "R"),
    (24, "yotta", "Y"),
    (21, "zetta", "Z"),
    (18, "exa", "E"),
    (15, "peta", "P"),
    (12, "tera", "T"),
    (9, "giga", "G"),
    (6, "mega", "M"),
    (3, "kilo", "k"),
    (2, "hecto", "h"),
    (1, "deca", "da"),
    (0, "", ""),
    (-1, "deci", "d"),
    (-2, "centi", "c"),
    (-3, "milli", "m"),
    (-6, "micro", "µ"),
    (-9, "nano", "n"),
    (-12, "pico", "p"),
    (-15, "femto", "f"),
    (-18, "atto", "a"),
    (-21, "zepto", "z"),
    (-24, "yocto", "y"),
    (-27, "ronto", "r"),
    (-30, "quecto", "q"),
];

pub struct Unit<'a, T> {
    pub value: T,
    pub prefix: PrefixTuple,
    pub suffix: &'a str,
}

fn get_prefix<T>(value: T) -> PrefixTuple
where
    T: Copy + PartialOrd + TryInto<f64>,
{
    let target_value: f64 = value.try_into().ok().unwrap_or(0.0).abs();

    // Find the largest prefix where the value is >= prefix_value
    for &prefix @ (exponent, _, _) in PREFIXES {
        let prefix_value: f64 = 10f64.powi(exponent);

        if target_value >= prefix_value {
            return prefix;
        }
    }

    // Default to base unit if no prefix matches
    (0, "", "")
}

impl<'a, T> Unit<'a, T>
where
    T: Copy + PartialOrd + TryInto<f64>,
{
    pub fn new(value: T, suffix: &'a str) -> Self {
        let prefix = get_prefix(value);
        Self {
            value,
            prefix,
            suffix,
        }
    }

    pub fn with_custom_prefix(value: T, prefix: PrefixTuple, suffix: &'a str) -> Self {
        Self {
            value,
            prefix,
            suffix,
        }
    }

    pub fn get_prefix(&self) -> PrefixTuple {
        self.prefix
    }

    pub fn format(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (exponent, _, symbol) = self.prefix;

        let scaled_value: f64 = self.value.try_into().ok().unwrap_or(0.0) / 10f64.powi(exponent);

        // Check if the value has a fractional part
        if scaled_value.fract() == 0.0 {
            write!(fmt, "{} {}{}", scaled_value, symbol, self.suffix)
        } else {
            write!(fmt, "{:.2} {}{}", scaled_value, symbol, self.suffix)
        }
    }

    pub fn format_full_name(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (exponent, name, _) = self.prefix;

        let scaled_value: f64 = self.value.try_into().ok().unwrap_or(0.0) / 10f64.powi(exponent);

        // Check if the value has a fractional part
        if scaled_value.fract() == 0.0 {
            write!(fmt, "{} {}{}", scaled_value, name, self.suffix)
        } else {
            write!(fmt, "{:.2} {}{}", scaled_value, name, self.suffix)
        }
    }
}

impl<T> Debug for Unit<'_, T>
where
    T: Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Unit")
            .field("value", &self.value)
            .field("suffix", &self.suffix)
            .finish()
    }
}

impl<T> Display for Unit<'_, T>
where
    T: Display + Copy + PartialOrd + TryInto<f64>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format(formatter)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::format;

    use super::*;

    #[test]
    fn test_unit_new() {
        let unit = Unit::new(1000.0, "m");
        assert_eq!(unit.value, 1000.0);
        assert_eq!(unit.suffix, "m");
    }

    #[test]
    fn test_get_prefix_base() {
        let prefix = get_prefix(1.0);
        assert_eq!(prefix, (0, "", ""));
    }

    #[test]
    fn test_get_prefix_kilo() {
        let prefix = get_prefix(1000.0);
        assert_eq!(prefix, (3, "kilo", "k"));
    }

    #[test]
    fn test_get_prefix_mega() {
        let prefix = get_prefix(1_000_000.0);
        assert_eq!(prefix, (6, "mega", "M"));
    }

    #[test]
    fn test_get_prefix_giga() {
        let prefix = get_prefix(1_000_000_000.0);
        assert_eq!(prefix, (9, "giga", "G"));
    }

    #[test]
    fn test_get_prefix_milli() {
        let prefix = get_prefix(0.001);
        assert_eq!(prefix, (-3, "milli", "m"));
    }

    #[test]
    fn test_get_prefix_micro() {
        let prefix = get_prefix(0.000001);
        assert_eq!(prefix, (-6, "micro", "µ"));
    }

    #[test]
    fn test_get_prefix_nano() {
        let prefix = get_prefix(0.000000001);
        assert_eq!(prefix, (-9, "nano", "n"));
    }

    #[test]
    fn test_unit_format() {
        let unit = Unit::new(1500.0, "m");
        assert_eq!(format!("{}", unit), "1.50 km");
    }

    #[test]
    fn test_unit_format_base() {
        let unit = Unit::new(5.0, "g");
        assert_eq!(format!("{}", unit), "5 g");
    }

    #[test]
    fn test_unit_format_milli() {
        let unit = Unit::new(0.005, "A");
        assert_eq!(format!("{}", unit), "5 mA");
    }

    #[test]
    fn test_unit_format_mega() {
        let unit = Unit::new(2_500_000.0, "B");
        assert_eq!(format!("{}", unit), "2.50 MB");
    }

    #[test]
    fn test_unit_debug() {
        let unit = Unit::new(42.0, "Hz");
        let debug_str = format!("{:?}", unit);
        assert!(debug_str.contains("value"));
        assert!(debug_str.contains("suffix"));
    }

    #[test]
    fn test_get_prefix_tera() {
        let prefix = get_prefix(1_000_000_000_000.0);
        assert_eq!(prefix, (12, "tera", "T"));
    }

    #[test]
    fn test_get_prefix_pico() {
        let prefix = get_prefix(0.000000000001);
        assert_eq!(prefix, (-12, "pico", "p"));
    }

    #[test]
    fn test_unit_format_small_value() {
        let unit = Unit::new(0.0000025, "F");
        assert_eq!(format!("{}", unit), "2.50 µF");
    }

    #[test]
    fn test_unit_format_large_value() {
        let unit = Unit::new(5_000_000_000.0, "Hz");
        assert_eq!(format!("{}", unit), "5 GHz");
    }
}
