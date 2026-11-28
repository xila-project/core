//! Xila's log subsystem.
//!
//! Code logs through the `log` crate's front end, re-exported here as
//! [`error!`], [`warning!`], [`information!`], [`debug!`] and [`trace!`]; one
//! [`LoggerTrait`] installed with [`initialize`] receives every message whose
//! level its [`LoggerTrait::enabled`] accepts, including those of third-party
//! crates that use `log` directly. The macros evaluate their arguments before
//! that check, so keep expensive work out of messages that are often filtered
//! out. Outside Core, use the re-export `xila::log`.
//!
//! # Levels
//!
//! [`Level`] has five levels, from the most to the least severe: `error`,
//! `warning`, `information`, `debug` and `trace`. [`LevelFilter`] adds `off`
//! and selects a level and every more severe one. Both print and parse as
//! those lowercase names (parsing ignores ASCII case), for options such as
//! `--log-level warning`.
//!
//! - `error`: an operation failed, and something the person or the system
//!   expected did not happen.
//! - `warning`: something unexpected happened, and the system recovered or
//!   carried on in a degraded way.
//! - `information`: rare, notable events, such as a subsystem starting. Not
//!   per request, per packet or per click.
//! - `debug`: details that help diagnose a problem.
//! - `trace`: very frequent details, such as every call of a hot path.
//!
//! # Message style
//!
//! - Write in English. Messages are for developers and are not localised, so
//!   never pass them through translation.
//! - Use sentence case: start with a capital letter, and keep the rest
//!   lowercase except names and acronyms (`Failed to mount the root file
//!   system`).
//! - Do not end a message with a period, an exclamation mark or a colon.
//! - Do not start a message with `Xila:` or with the subsystem name (`UDP:`,
//!   `WASI:`): the target, the module path by default, already says where it
//!   comes from.
//! - Show errors with their `Display` text (`{error}`), not with `{:?}`.
//! - Put values after the text that names them (`Failed to open {path}:
//!   {error}`), with no space before a colon.
//! - Never log secrets or personal data: typed or pasted characters,
//!   passwords, keys, random generator output, user names or full response
//!   bodies. Log a count or a result instead.

#![no_std]

extern crate alloc;

use alloc::fmt;
use core::cmp::Ordering;
use core::str::FromStr;
use log_external::Log;
use log_external::Metadata;
pub use log_external::{debug, error, info as information, trace, warn as warning};
use synchronization::once_lock::OnceLock;

const BOLD: &str = "\x1b[0;1m";
const RED: &str = "\x1b[0;41m";
const YELLOW: &str = "\x1b[0;43m";
const BLUE: &str = "\x1b[0;46m";
const GREEN: &str = "\x1b[0;42m";
const GREY: &str = "\x1b[0;100m";
const RESET: &str = "\x1b[0m";

/// The severity of a log message, from the most severe ([`Level::Error`]) to
/// the least ([`Level::Trace`]).
///
/// Levels are ordered by verbosity, like the `log` crate's: `Error` is the
/// smallest and `Trace` the largest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warning,
    Information,
    Debug,
    Trace,
}

#[allow(non_upper_case_globals)]
impl Level {
    /// Former name of [`Level::Warning`], kept for one release.
    ///
    /// It works in expressions and patterns, but as an associated constant it
    /// cannot be imported: `use log::Level::Warn` no longer compiles.
    #[deprecated(note = "use `Level::Warning`")]
    pub const Warn: Level = Level::Warning;

    /// Former name of [`Level::Information`], kept for one release.
    ///
    /// It works in expressions and patterns, but as an associated constant it
    /// cannot be imported: `use log::Level::Info` no longer compiles.
    #[deprecated(note = "use `Level::Information`")]
    pub const Info: Level = Level::Information;
}

impl Level {
    /// Every level, from the most to the least severe.
    pub const ALL: [Level; 5] = [
        Level::Error,
        Level::Warning,
        Level::Information,
        Level::Debug,
        Level::Trace,
    ];

    /// The lowercase name of the level, as it prints and parses.
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Information => "information",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad(self.as_str())
    }
}

impl FromStr for Level {
    type Err = ParseLevelError;

    /// Parses a level name, ignoring ASCII case. Abbreviations such as `warn`
    /// or `info` are refused.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Level::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(name))
            .ok_or(ParseLevelError(()))
    }
}

/// The most verbose level that passes a filter, or [`LevelFilter::Off`] to let
/// nothing pass.
///
/// A filter lets a level pass when the level is at most as verbose as the
/// filter: [`LevelFilter::Warning`] lets errors and warnings pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LevelFilter {
    Off = 0,
    Error,
    Warning,
    Information,
    Debug,
    Trace,
}

impl LevelFilter {
    /// Every filter, from [`LevelFilter::Off`] to the most verbose.
    pub const ALL: [LevelFilter; 6] = [
        LevelFilter::Off,
        LevelFilter::Error,
        LevelFilter::Warning,
        LevelFilter::Information,
        LevelFilter::Debug,
        LevelFilter::Trace,
    ];

    /// The lowercase name of the filter, as it prints and parses.
    pub const fn as_str(self) -> &'static str {
        match self.level() {
            Some(level) => level.as_str(),
            None => "off",
        }
    }

    /// The most verbose level that passes, or `None` for [`LevelFilter::Off`].
    pub const fn level(self) -> Option<Level> {
        match self {
            LevelFilter::Off => None,
            LevelFilter::Error => Some(Level::Error),
            LevelFilter::Warning => Some(Level::Warning),
            LevelFilter::Information => Some(Level::Information),
            LevelFilter::Debug => Some(Level::Debug),
            LevelFilter::Trace => Some(Level::Trace),
        }
    }

    /// Whether messages of `level` pass this filter.
    pub const fn enables(self, level: Level) -> bool {
        level as u8 <= self as u8
    }
}

impl From<Level> for LevelFilter {
    fn from(level: Level) -> Self {
        match level {
            Level::Error => LevelFilter::Error,
            Level::Warning => LevelFilter::Warning,
            Level::Information => LevelFilter::Information,
            Level::Debug => LevelFilter::Debug,
            Level::Trace => LevelFilter::Trace,
        }
    }
}

impl fmt::Display for LevelFilter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.pad(self.as_str())
    }
}

impl FromStr for LevelFilter {
    type Err = ParseLevelError;

    /// Parses `off` or a level name, ignoring ASCII case. Abbreviations such
    /// as `warn` or `info` are refused.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        LevelFilter::ALL
            .into_iter()
            .find(|filter| filter.as_str().eq_ignore_ascii_case(name))
            .ok_or(ParseLevelError(()))
    }
}

impl PartialEq<LevelFilter> for Level {
    fn eq(&self, other: &LevelFilter) -> bool {
        *self as u8 == *other as u8
    }
}

impl PartialOrd<LevelFilter> for Level {
    fn partial_cmp(&self, other: &LevelFilter) -> Option<Ordering> {
        Some((*self as u8).cmp(&(*other as u8)))
    }
}

impl PartialEq<Level> for LevelFilter {
    fn eq(&self, other: &Level) -> bool {
        *self as u8 == *other as u8
    }
}

impl PartialOrd<Level> for LevelFilter {
    fn partial_cmp(&self, other: &Level) -> Option<Ordering> {
        Some((*self as u8).cmp(&(*other as u8)))
    }
}

/// The text given to [`Level::from_str`] or [`LevelFilter::from_str`] is not a
/// level name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLevelError(());

impl fmt::Display for ParseLevelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Unknown log level")
    }
}

impl core::error::Error for ParseLevelError {}

pub struct Record<'a> {
    pub level: Level,
    pub target: &'a str,
    pub arguments: fmt::Arguments<'a>,
}

impl<'a> Record<'a> {
    fn from_log_external(record: &log_external::Record<'a>) -> Self {
        Self {
            level: Level::from(record.level()),
            target: record.target(),
            arguments: *record.args(),
        }
    }
}

impl From<log_external::Level> for Level {
    fn from(level: log_external::Level) -> Self {
        match level {
            log_external::Level::Error => Level::Error,
            log_external::Level::Warn => Level::Warning,
            log_external::Level::Info => Level::Information,
            log_external::Level::Debug => Level::Debug,
            log_external::Level::Trace => Level::Trace,
        }
    }
}

pub trait LoggerTrait: Send + Sync {
    fn enabled(&self, level: Level) -> bool;

    fn write(&self, arguments: fmt::Arguments);

    fn log(&self, record: &Record) {
        let (letter, color) = match record.level {
            Level::Error => ("E", RED),
            Level::Warning => ("W", YELLOW),
            Level::Information => ("I", BLUE),
            Level::Debug => ("D", GREEN),
            Level::Trace => ("T", GREY),
        };

        self.write(format_args!(
            "{} {} {} | {}{}{} | {}",
            color, letter, RESET, BOLD, record.target, RESET, record.arguments
        ))
    }

    fn flush(&self) {
        // Implement flush logic if needed, e.g., flushing buffers to a file or console
    }
}

struct Logger(&'static dyn LoggerTrait);

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.0.enabled(Level::from(metadata.level()))
    }

    fn log(&self, record: &log_external::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        self.0.log(&Record::from_log_external(record));
    }

    fn flush(&self) {
        self.0.flush();
    }
}

static LOGGER_INSTANCE: OnceLock<Logger> = OnceLock::new();

pub fn initialize(logger: &'static dyn LoggerTrait) -> Result<(), log_external::SetLoggerError> {
    let log = LOGGER_INSTANCE.get_or_init(|| Logger(logger));

    if install(log).is_err() {
        logger.log(&Record {
            level: Level::Error,
            target: "log",
            arguments: format_args!("Logger has already been initialized"),
        });
    }

    Ok(())
}

/// Register `log` as the global logger and enable every level.
#[cfg(target_has_atomic = "ptr")]
fn install(log: &'static Logger) -> Result<(), log_external::SetLoggerError> {
    let result = log_external::set_logger(log);
    log_external::set_max_level(log_external::LevelFilter::Trace);
    result
}

/// Register `log` as the global logger and enable every level.
///
/// Without pointer compare-and-swap, `log` only offers racy setters. One
/// critical section serializes every call made through this crate.
#[cfg(not(target_has_atomic = "ptr"))]
fn install(log: &'static Logger) -> Result<(), log_external::SetLoggerError> {
    critical_section::with(|_| {
        // SAFETY: the critical section excludes every other `initialize`
        // call, the only place Xila sets the logger and maximum level. Firmware
        // and HAL crates must not call `log::set_logger_racy` or
        // `log::set_max_level_racy` themselves outside this critical section.
        // Logging concurrently with these setters is allowed by `log`.
        unsafe {
            let result = log_external::set_logger_racy(log);
            log_external::set_max_level_racy(log_external::LevelFilter::Trace);
            result
        }
    })
}

pub fn is_initialized() -> bool {
    LOGGER_INSTANCE.try_get().is_some()
}

/// Copy the most recent complete log lines into the caller's buffer without
/// blocking. Returns zero when no ring buffer is installed or a write is in
/// progress.
pub fn snapshot_tail(output: &mut [u8]) -> usize {
    // SAFETY: The slice is valid for writes for its lifetime.
    unsafe { snapshot_tail_raw(output.as_mut_ptr(), output.len()) }
}

/// Raw variant of [`snapshot_tail`] for callers that must not create a Rust
/// reference to the destination, for example the retained crash slot of
/// another core.
///
/// # Safety
/// `output..output.add(capacity)` must be valid for writes.
pub unsafe fn snapshot_tail_raw(_output: *mut u8, _capacity: usize) -> usize {
    // The log ring buffer is tracked in xila-project/core#840.
    0
}
