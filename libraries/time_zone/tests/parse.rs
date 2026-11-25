//! Parsing of POSIX TZ strings: valid strings, and the error of each invalid
//! one.

use time_zone::{ParseError, RuleKind, TimeZone};

#[test]
fn simple_zones_without_daylight_saving() {
    let utc = TimeZone::parse("UTC0").unwrap();
    assert_eq!(utc.standard_abbreviation(), "UTC");
    assert_eq!(utc.standard_offset_seconds(), 0);
    assert!(!utc.has_daylight_saving());

    let kolkata = TimeZone::parse("IST-5:30").unwrap();
    assert_eq!(kolkata.standard_abbreviation(), "IST");
    assert_eq!(kolkata.standard_offset_seconds(), 5 * 3600 + 30 * 60);
    assert!(!kolkata.has_daylight_saving());

    let tokyo = TimeZone::parse("JST-9").unwrap();
    assert_eq!(tokyo.standard_offset_seconds(), 9 * 3600);

    // An offset of exactly 24 hours is allowed. POSIX offsets are positive
    // west of UTC, so `XXX-24` is 24 hours east and `XXX24` is 24 hours west.
    let zone = TimeZone::parse("XXX-24").unwrap();
    assert_eq!(zone.standard_offset_seconds(), 24 * 3600);
    let zone = TimeZone::parse("XXX24:00:00").unwrap();
    assert_eq!(zone.standard_offset_seconds(), -24 * 3600);

    let chatham = TimeZone::parse("<+1245>-12:45<+1345>,M9.5.0/2:45,M4.1.0/3:45").unwrap();
    assert_eq!(chatham.standard_abbreviation(), "+1245");
    assert_eq!(chatham.standard_offset_seconds(), 12 * 3600 + 45 * 60);
    assert_eq!(chatham.daylight_saving_abbreviation(), Some("+1345"));
    // The daylight saving offset is omitted, so it is one hour ahead of the
    // standard one.
    assert_eq!(
        chatham.daylight_saving_offset_seconds(),
        Some(13 * 3600 + 45 * 60)
    );
    assert_eq!(chatham.start_rule().time_seconds(), 2 * 3600 + 45 * 60);
    assert_eq!(chatham.end_rule().time_seconds(), 3 * 3600 + 45 * 60);
}

#[test]
fn zone_with_daylight_saving_rules() {
    let paris = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
    assert_eq!(paris.standard_abbreviation(), "CET");
    assert_eq!(paris.standard_offset_seconds(), 3600);
    assert_eq!(paris.daylight_saving_abbreviation(), Some("CEST"));
    assert_eq!(paris.daylight_saving_offset_seconds(), Some(7200));
    assert_eq!(paris.start_rule().kind(), RuleKind::MonthWeekDay);
    assert_eq!(
        (
            paris.start_rule().month(),
            paris.start_rule().week(),
            paris.start_rule().weekday()
        ),
        (3, 5, 0)
    );
    // A rule without a time defaults to 02:00:00 and is not explicit.
    assert_eq!(paris.start_rule().time_seconds(), 2 * 3600);
    assert!(!paris.start_rule().has_explicit_time());
    assert_eq!(paris.end_rule().time_seconds(), 3 * 3600);
    assert!(paris.end_rule().has_explicit_time());
    assert_eq!(
        (
            paris.end_rule().month(),
            paris.end_rule().week(),
            paris.end_rule().weekday()
        ),
        (10, 5, 0)
    );
}

#[test]
fn daylight_saving_with_explicit_offset_and_default_rules() {
    // The daylight saving offset is given, and no rules means the United
    // States rules.
    let zone = TimeZone::parse("EST5EDT4,M3.2.0,M11.1.0").unwrap();
    assert_eq!(zone.standard_offset_seconds(), -5 * 3600);
    assert_eq!(zone.daylight_saving_offset_seconds(), Some(-4 * 3600));

    let default_rules = TimeZone::parse("EST5EDT").unwrap();
    assert_eq!(
        default_rules.daylight_saving_offset_seconds(),
        Some(-4 * 3600)
    );
    assert_eq!(
        (
            default_rules.start_rule().month(),
            default_rules.start_rule().week(),
            default_rules.start_rule().weekday()
        ),
        (3, 2, 0)
    );
    assert_eq!(default_rules.start_rule().time_seconds(), 2 * 3600);
    assert!(!default_rules.start_rule().has_explicit_time());
    assert_eq!(
        (
            default_rules.end_rule().month(),
            default_rules.end_rule().week(),
            default_rules.end_rule().weekday()
        ),
        (11, 1, 0)
    );

    // An explicit /2 stays explicit.
    let explicit = TimeZone::parse("EST5EDT,M3.2.0/2,M11.1.0/2").unwrap();
    assert!(explicit.start_rule().has_explicit_time());
    assert_eq!(format!("{:?}", explicit.start_rule()), "M3.2.0/2");
}

#[test]
fn rule_forms_and_signed_times() {
    // J rule and zero based rule, with times above 24 hours and negative.
    let zone = TimeZone::parse("AAA3BBB,J1/0,J365/25").unwrap();
    assert_eq!(zone.start_rule().kind(), RuleKind::JulianNoLeap);
    assert_eq!(zone.start_rule().julian_day(), 1);
    assert_eq!(zone.start_rule().time_seconds(), 0);
    assert_eq!(zone.end_rule().julian_day(), 365);
    assert_eq!(zone.end_rule().time_seconds(), 25 * 3600);

    let zone = TimeZone::parse("CCC3DDD,0,364").unwrap();
    assert_eq!(zone.start_rule().kind(), RuleKind::ZeroBasedDay);
    assert_eq!(zone.start_rule().zero_based_day(), 0);
    assert_eq!(zone.end_rule().zero_based_day(), 364);

    let zone = TimeZone::parse("EEE-2FFF,M3.4.4/26,M10.5.0").unwrap();
    assert_eq!(zone.start_rule().time_seconds(), 26 * 3600);
    assert_eq!(zone.end_rule().time_seconds(), 2 * 3600);
    assert!(!zone.end_rule().has_explicit_time());

    let zone = TimeZone::parse("GGG2HHH,M3.5.0/-1,M10.5.0/0").unwrap();
    assert_eq!(zone.start_rule().time_seconds(), -3600);
    assert_eq!(zone.end_rule().time_seconds(), 0);

    // The largest POSIX 2024 time: 167:59:59, and its negative.
    let zone = TimeZone::parse("III2JJJ,M3.5.0/167:59:59,M10.5.0/-167:59:59").unwrap();
    assert_eq!(zone.start_rule().time_seconds(), 167 * 3600 + 59 * 60 + 59);
    assert_eq!(zone.end_rule().time_seconds(), -(167 * 3600 + 59 * 60 + 59));

    // Signs and seconds in offsets.
    let zone = TimeZone::parse("<+0545>-5:45").unwrap();
    assert_eq!(zone.standard_offset_seconds(), 5 * 3600 + 45 * 60);
    let zone = TimeZone::parse("KKK+2:30:15").unwrap();
    assert_eq!(zone.standard_offset_seconds(), -(2 * 3600 + 30 * 60 + 15));
}

#[test]
fn negative_daylight_saving_offset() {
    // Europe/Dublin: the daylight saving offset is smaller than the standard
    // one.
    let dublin = TimeZone::parse("IST-1GMT0,M10.5.0,M3.5.0/1").unwrap();
    assert_eq!(dublin.standard_abbreviation(), "IST");
    assert_eq!(dublin.standard_offset_seconds(), 3600);
    assert_eq!(dublin.daylight_saving_abbreviation(), Some("GMT"));
    assert_eq!(dublin.daylight_saving_offset_seconds(), Some(0));
}

#[test]
fn names_of_exactly_seven_bytes_are_accepted() {
    // `ABBREVIATION_MAX` is 7: the longest name that fits must parse, and the
    // next length must fail (see `invalid_strings`).
    let unquoted = TimeZone::parse("ABCDEFG1").unwrap();
    assert_eq!(unquoted.standard_abbreviation(), "ABCDEFG");
    let quoted = TimeZone::parse("<abcdefg>1").unwrap();
    assert_eq!(quoted.standard_abbreviation(), "abcdefg");
    let daylight_saving = TimeZone::parse("ABCDEFG1HIJKLMN").unwrap();
    assert_eq!(
        daylight_saving.daylight_saving_abbreviation(),
        Some("HIJKLMN")
    );
}

#[test]
fn invalid_strings() {
    let cases: &[(&str, ParseError)] = &[
        ("", ParseError::Empty),
        ("UTC", ParseError::MissingOffset),
        ("5EDT", ParseError::MissingName),
        ("-1CEST", ParseError::MissingName),
        ("AB1", ParseError::NameTooShort),
        ("AB-1", ParseError::NameTooShort),
        ("<AB>1", ParseError::NameTooShort),
        ("<A>1", ParseError::NameTooShort),
        ("<ABC", ParseError::UnterminatedName),
        ("<a,b>-3", ParseError::InvalidNameCharacter),
        ("<a b c>-3", ParseError::InvalidNameCharacter),
        ("<é é>-3", ParseError::InvalidNameCharacter),
        ("<abc>-3<d,e>", ParseError::InvalidNameCharacter),
        ("<abcdefgh>1", ParseError::NameTooLong),
        ("ABCDEFGH-1", ParseError::NameTooLong),
        ("CET", ParseError::MissingOffset),
        ("CET-", ParseError::MissingOffset),
        ("CET-x", ParseError::MissingOffset),
        ("CET-1:", ParseError::InvalidOffset),
        ("CET-1:2", ParseError::InvalidOffset),
        ("CET-1:2:3:4", ParseError::InvalidOffset),
        ("CET-1:99", ParseError::InvalidOffset),
        ("CET-1:00:99", ParseError::InvalidOffset),
        ("CET--1", ParseError::MissingOffset),
        ("CET-25", ParseError::OffsetOutOfRange),
        ("CET-24:00:01", ParseError::OffsetOutOfRange),
        ("CET-1CEST,M3.5.0", ParseError::MissingEndRule),
        ("CET-1CEST,", ParseError::InvalidRule),
        ("CET-1CEST,,M10.5.0", ParseError::InvalidRule),
        (
            "CET-1CEST,M3.5.0,M10.5.0/3,M1.1.0",
            ParseError::TrailingCharacters,
        ),
        ("CET-1CEST,M3.5.0;M10.5.0", ParseError::MissingEndRule),
        (
            "EST5,M3.2.0,M11.1.0",
            ParseError::RulesWithoutDaylightSaving,
        ),
        ("CET-1CEST,X3.5.0,M10.5.0", ParseError::InvalidRule),
        ("CET-1CEST,M0.5.0,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,M13.5.0,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,M3.0.0,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,M3.6.0,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,M3.5.7,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,J0,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,J366,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,366,M10.5.0", ParseError::RuleOutOfRange),
        ("CET-1CEST,M3.5,M10.5.0", ParseError::InvalidRule),
        ("CET-1CEST,M3.5.0/,M10.5.0", ParseError::InvalidTime),
        ("CET-1CEST,M3.5.0/2:,M10.5.0", ParseError::InvalidTime),
        ("CET-1CEST,M3.5.0/2:60,M10.5.0", ParseError::InvalidTime),
        ("CET-1CEST,M3.5.0/2:00:60,M10.5.0", ParseError::InvalidTime),
        ("CET-1CEST,M3.5.0/168,M10.5.0", ParseError::TimeOutOfRange),
        ("CET-1CEST,M3.5.0/-168,M10.5.0", ParseError::TimeOutOfRange),
        ("CET-1CEST,M3.5.0/25C,M10.5.0", ParseError::MissingEndRule),
        ("CET-1CEST M3.5.0,M10.5.0", ParseError::TrailingCharacters),
    ];
    for (input, expected) in cases {
        assert_eq!(
            TimeZone::parse(input).unwrap_err(),
            *expected,
            "input: {input:?}"
        );
    }
}

#[test]
fn parsed_value_is_copy_and_32_bytes() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<TimeZone>();
    // The two abbreviation buffers, two offsets and two rules, with no
    // padding: 8 + 8 + 4 + 4 + 4 + 4.
    assert_eq!(core::mem::size_of::<TimeZone>(), 32);
    assert_eq!(core::mem::size_of::<time_zone::Rule>(), 4);
    let zone = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
    let copy = zone;
    assert_eq!(zone, copy);
    // The value owns its abbreviations: it does not borrow the input.
    let owned = TimeZone::parse(&String::from("CET-1CEST,M3.5.0,M10.5.0/3")).unwrap();
    assert_eq!(owned.standard_abbreviation(), "CET");
}

#[test]
fn error_display_is_not_empty() {
    for error in [
        ParseError::Empty,
        ParseError::MissingName,
        ParseError::NameTooShort,
        ParseError::InvalidNameCharacter,
        ParseError::NameTooLong,
        ParseError::UnterminatedName,
        ParseError::MissingOffset,
        ParseError::InvalidOffset,
        ParseError::OffsetOutOfRange,
        ParseError::RulesWithoutDaylightSaving,
        ParseError::InvalidRule,
        ParseError::RuleOutOfRange,
        ParseError::InvalidTime,
        ParseError::TimeOutOfRange,
        ParseError::MissingEndRule,
        ParseError::TrailingCharacters,
    ] {
        assert!(!error.to_string().is_empty());
    }
}
