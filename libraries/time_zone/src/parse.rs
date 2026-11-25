//! Parser for POSIX TZ strings, the `TZ` format of POSIX `tzset`:
//! `std offset [dst [offset] [,rule,rule]]`.
//!
//! Names are either at least three letters (`CET`) or quoted in angle brackets
//! (`<+03>`). Offsets are `[+-]hh[:mm[:ss]]` within 24 hours. Rules are
//! `Mm.w.d[/time]`, `Jn[/time]` or `n[/time]`; a rule time is a signed
//! `[+-]hh[:mm[:ss]]` within the POSIX 2024 range of 167 hours, and it defaults
//! to 02:00:00. A DST name without rules uses the United States rules
//! `M3.2.0/2,M11.1.0/2`, and a DST offset without a value is one hour ahead of
//! the standard offset.

use crate::rule::{Rule, RuleKind};
use crate::zone::TimeZone;
use core::fmt;

/// Default time of a rule without `/time`: 02:00:00.
const DEFAULT_RULE_TIME: i32 = 2 * 3600;

/// Default start rule when a DST name is given without rules: `M3.2.0`.
// M3.2.0, packed as ((3 - 1) * 5 + (2 - 1)) * 7 + 0.
const DEFAULT_START: Rule = Rule::new(RuleKind::MonthWeekDay, 77, DEFAULT_RULE_TIME);

/// Default end rule when a DST name is given without rules: `M11.1.0`.
// M11.1.0, packed as ((11 - 1) * 5 + (1 - 1)) * 7 + 0.
const DEFAULT_END: Rule = Rule::new(RuleKind::MonthWeekDay, 350, DEFAULT_RULE_TIME);

/// The largest zone offset of POSIX: 24 hours.
const MAX_OFFSET_SECONDS: i64 = 24 * 3600;

/// The largest rule time of POSIX 2024: 167:59:59.
const MAX_RULE_TIME_SECONDS: i64 = 167 * 3600 + 59 * 60 + 59;

/// Why a POSIX TZ string was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ParseError {
    /// The string is empty.
    Empty,
    /// The string does not start with a zone name.
    MissingName,
    /// A zone name has fewer than three bytes.
    NameTooShort,
    /// A quoted name is not closed with `>`.
    UnterminatedName,
    /// The standard offset is missing.
    MissingOffset,
    /// The standard offset is malformed.
    InvalidOffset,
    /// The standard offset is larger than 24 hours.
    OffsetOutOfRange,
    /// Rules are given without a DST name.
    RulesWithoutDst,
    /// A transition rule is malformed.
    InvalidRule,
    /// A number of a rule is out of its range.
    RuleOutOfRange,
    /// A rule time is malformed.
    InvalidTime,
    /// A rule time is larger than 167 hours.
    TimeOutOfRange,
    /// The comma-separated rule pair has only one rule.
    MissingEndRule,
    /// Extra characters follow a complete TZ string.
    TrailingCharacters,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ParseError::Empty => "the TZ string is empty",
            ParseError::MissingName => "a zone name is missing",
            ParseError::NameTooShort => "a zone name has fewer than three bytes",
            ParseError::UnterminatedName => "a quoted zone name is not closed",
            ParseError::MissingOffset => "the standard offset is missing",
            ParseError::InvalidOffset => "the standard offset is malformed",
            ParseError::OffsetOutOfRange => "the standard offset is larger than 24 hours",
            ParseError::RulesWithoutDst => "rules are given without a DST name",
            ParseError::InvalidRule => "a transition rule is malformed",
            ParseError::RuleOutOfRange => "a number of a rule is out of range",
            ParseError::InvalidTime => "a rule time is malformed",
            ParseError::TimeOutOfRange => "a rule time is larger than 167 hours",
            ParseError::MissingEndRule => "the rule pair has only one rule",
            ParseError::TrailingCharacters => "extra characters follow the TZ string",
        })
    }
}

impl core::error::Error for ParseError {}

/// Parse a POSIX TZ string into a [`TimeZone`] that borrows its abbreviations
/// from the input.
pub(crate) fn parse(input: &str) -> Result<TimeZone<'_>, ParseError> {
    Parser { input, pos: 0 }.parse_zone()
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Read up to `max` decimal digits. Returns the value and how many digits
    /// were read, or None when there is no digit.
    fn digits(&mut self, max: usize) -> Option<(i64, usize)> {
        let start = self.pos;
        let bytes = self.input.as_bytes();
        let mut value: i64 = 0;
        while self.pos < bytes.len() && self.pos - start < max && bytes[self.pos].is_ascii_digit() {
            value = value * 10 + i64::from(bytes[self.pos] - b'0');
            self.pos += 1;
        }
        if self.pos == start {
            None
        } else {
            Some((value, self.pos - start))
        }
    }

    /// Read `[+-]hh[:mm[:ss]]` and return the signed value in seconds.
    fn parse_hms(&mut self, missing: ParseError, invalid: ParseError) -> Result<i64, ParseError> {
        let sign = match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                -1
            }
            Some(b'+') => {
                self.pos += 1;
                1
            }
            _ => 1,
        };
        let (hours, _) = self.digits(3).ok_or(missing)?;
        let mut total = hours * 3600;
        if self.peek() == Some(b':') {
            self.pos += 1;
            let (minutes, count) = self.digits(2).ok_or(invalid)?;
            if count != 2 {
                return Err(invalid);
            }
            total += minutes * 60;
            if self.peek() == Some(b':') {
                self.pos += 1;
                let (seconds, count) = self.digits(2).ok_or(invalid)?;
                if count != 2 {
                    return Err(invalid);
                }
                total += seconds;
            }
        }
        Ok(sign * total)
    }

    /// Read a zone name: `<...>` or at least three letters.
    fn parse_name(&mut self) -> Result<&'a str, ParseError> {
        if self.peek() == Some(b'<') {
            self.pos += 1;
            let start = self.pos;
            loop {
                match self.peek() {
                    None => return Err(ParseError::UnterminatedName),
                    Some(b'>') => {
                        let name = &self.input[start..self.pos];
                        self.pos += 1;
                        if name.len() < 3 {
                            return Err(ParseError::NameTooShort);
                        }
                        return Ok(name);
                    }
                    Some(_) => self.pos += 1,
                }
            }
        }
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let name = &self.input[start..self.pos];
        if name.is_empty() {
            return Err(ParseError::MissingName);
        }
        if name.len() < 3 {
            return Err(ParseError::NameTooShort);
        }
        Ok(name)
    }

    /// Read a zone offset and return it in POSIX form, positive west of UTC.
    fn parse_zone_offset(&mut self) -> Result<i64, ParseError> {
        let value = self.parse_hms(ParseError::MissingOffset, ParseError::InvalidOffset)?;
        if value.abs() > MAX_OFFSET_SECONDS {
            return Err(ParseError::OffsetOutOfRange);
        }
        Ok(value)
    }

    /// Read one transition rule.
    fn parse_rule(&mut self) -> Result<Rule, ParseError> {
        let (kind, date) = match self.peek() {
            Some(b'M') => {
                self.pos += 1;
                let (month, _) = self.digits(2).ok_or(ParseError::InvalidRule)?;
                if !(1..=12).contains(&month) {
                    return Err(ParseError::RuleOutOfRange);
                }
                if !self.eat(b'.') {
                    return Err(ParseError::InvalidRule);
                }
                let (week, _) = self.digits(1).ok_or(ParseError::InvalidRule)?;
                if !(1..=5).contains(&week) {
                    return Err(ParseError::RuleOutOfRange);
                }
                if !self.eat(b'.') {
                    return Err(ParseError::InvalidRule);
                }
                let (weekday, _) = self.digits(1).ok_or(ParseError::InvalidRule)?;
                if weekday > 6 {
                    return Err(ParseError::RuleOutOfRange);
                }
                let code = ((month - 1) * 5 + (week - 1)) * 7 + weekday;
                (RuleKind::MonthWeekDay, code as u16)
            }
            Some(b'J') => {
                self.pos += 1;
                let (day, _) = self.digits(3).ok_or(ParseError::InvalidRule)?;
                if !(1..=365).contains(&day) {
                    return Err(ParseError::RuleOutOfRange);
                }
                (RuleKind::JulianNoLeap, (day - 1) as u16)
            }
            Some(c) if c.is_ascii_digit() => {
                let (day, _) = self.digits(3).ok_or(ParseError::InvalidRule)?;
                if day > 365 {
                    return Err(ParseError::RuleOutOfRange);
                }
                (RuleKind::ZeroBasedDay, day as u16)
            }
            _ => return Err(ParseError::InvalidRule),
        };
        let time = if self.eat(b'/') {
            let value = self.parse_hms(ParseError::InvalidTime, ParseError::InvalidTime)?;
            if value.abs() > MAX_RULE_TIME_SECONDS {
                return Err(ParseError::TimeOutOfRange);
            }
            value as i32
        } else {
            DEFAULT_RULE_TIME
        };
        Ok(Rule::new(kind, date, time))
    }

    fn parse_zone(&mut self) -> Result<TimeZone<'a>, ParseError> {
        if self.input.is_empty() {
            return Err(ParseError::Empty);
        }
        let std_abbr = self.parse_name()?;
        // POSIX offsets are positive west of UTC; store seconds east of UTC.
        let std_offset = -self.parse_zone_offset()? as i32;
        if self.pos >= self.input.len() {
            return Ok(TimeZone::fixed(std_abbr, std_offset));
        }
        if self.peek() == Some(b',') {
            return Err(ParseError::RulesWithoutDst);
        }
        let dst_abbr = self.parse_name()?;
        let next_starts_offset = match self.peek() {
            Some(b'+') | Some(b'-') => true,
            Some(c) => c.is_ascii_digit(),
            None => false,
        };
        let dst_offset = if next_starts_offset {
            -self.parse_zone_offset()? as i32
        } else {
            std_offset + 3600
        };
        let (start, end) = if self.eat(b',') {
            let start = self.parse_rule()?;
            if !self.eat(b',') {
                return Err(ParseError::MissingEndRule);
            }
            let end = self.parse_rule()?;
            (start, end)
        } else {
            (DEFAULT_START, DEFAULT_END)
        };
        if self.pos < self.input.len() {
            return Err(ParseError::TrailingCharacters);
        }
        Ok(TimeZone::with_dst(
            std_abbr, std_offset, dst_abbr, dst_offset, start, end,
        ))
    }
}
