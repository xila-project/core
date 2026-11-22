//! Level names, ordering and filters.

use log::{Level, LevelFilter};

#[test]
fn levels_print_and_parse_as_their_full_names() {
    let names = ["error", "warning", "information", "debug", "trace"];
    for (level, name) in Level::ALL.into_iter().zip(names) {
        assert_eq!(level.as_str(), name);
        assert_eq!(level.to_string(), name);
        assert_eq!(name.parse::<Level>(), Ok(level));
        assert_eq!(name.to_uppercase().parse::<Level>(), Ok(level));
    }
    assert_eq!(format!("{:>9}", Level::Debug), "    debug");
}

#[test]
fn level_parsing_refuses_abbreviations_and_off() {
    for name in ["warn", "info", "err", "dbg", "off", "", " error"] {
        assert!(name.parse::<Level>().is_err(), "{name:?} parsed");
    }
}

#[test]
fn filters_print_and_parse_as_their_full_names() {
    let names = ["off", "error", "warning", "information", "debug", "trace"];
    for (filter, name) in LevelFilter::ALL.into_iter().zip(names) {
        assert_eq!(filter.as_str(), name);
        assert_eq!(filter.to_string(), name);
        assert_eq!(name.parse::<LevelFilter>(), Ok(filter));
    }
    assert!("information".parse::<LevelFilter>().is_ok());
    assert!("info".parse::<LevelFilter>().is_err());
    assert_eq!(
        "warn".parse::<LevelFilter>().unwrap_err().to_string(),
        "Unknown log level"
    );
}

#[test]
fn levels_are_ordered_by_verbosity() {
    assert!(Level::Error < Level::Warning);
    assert!(Level::Warning < Level::Information);
    assert!(Level::Information < Level::Debug);
    assert!(Level::Debug < Level::Trace);
    assert!(LevelFilter::Off < LevelFilter::Error);
    assert!(Level::Error > LevelFilter::Off);
    assert!(Level::Warning <= LevelFilter::Warning);
    assert!(Level::Information > LevelFilter::Warning);
    assert_eq!(Level::Debug, LevelFilter::Debug);
}

#[test]
fn filters_enable_their_level_and_more_severe_ones() {
    for level in Level::ALL {
        assert!(!LevelFilter::Off.enables(level));
        assert!(LevelFilter::Trace.enables(level));
        assert_eq!(LevelFilter::from(level).level(), Some(level));
        assert!(LevelFilter::from(level).enables(level));
    }
    assert_eq!(LevelFilter::Off.level(), None);
    assert!(LevelFilter::Warning.enables(Level::Error));
    assert!(LevelFilter::Warning.enables(Level::Warning));
    assert!(!LevelFilter::Warning.enables(Level::Information));
}

#[test]
#[allow(deprecated)]
fn former_level_names_still_match() {
    let letter = |level| match level {
        Level::Error => 'E',
        Level::Warn => 'W',
        Level::Info => 'I',
        Level::Debug => 'D',
        Level::Trace => 'T',
    };
    assert_eq!(letter(Level::Warning), 'W');
    assert_eq!(letter(Level::Information), 'I');
    assert_eq!(Level::Warn, Level::Warning);
    assert_eq!(Level::Info, Level::Information);
}
