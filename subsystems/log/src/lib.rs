#![no_std]

extern crate alloc;

use alloc::fmt;
use log_external::Log;
use log_external::Metadata;
use log_external::set_logger;
use log_external::set_max_level;
pub use log_external::{debug, error, info as information, trace, warn as warning};
use synchronization::once_lock::OnceLock;

const BOLD: &str = "\x1b[0;1m";
const RED: &str = "\x1b[0;41m";
const YELLOW: &str = "\x1b[0;43m";
const BLUE: &str = "\x1b[0;46m";
const GREEN: &str = "\x1b[0;42m";
const GREY: &str = "\x1b[0;100m";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error = 1,
    Warn,
    Info,
    Debug,
    Trace,
}

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
            log_external::Level::Warn => Level::Warn,
            log_external::Level::Info => Level::Info,
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
            Level::Warn => ("W", YELLOW),
            Level::Info => ("I", BLUE),
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

    if set_logger(log).is_err() {
        logger.log(&Record {
            level: Level::Error,
            target: "log",
            arguments: format_args!("Logger has already been initialized."),
        });
    }

    set_max_level(log_external::LevelFilter::Trace);
    Ok(())
}

pub fn is_initialized() -> bool {
    LOGGER_INSTANCE.try_get().is_some()
}

pub fn test_write(logger: &impl LoggerTrait) {
    for i in 0..5 {
        logger.write(format_args!("This is a test message number {i}."));
    }
}

pub fn test_log(logger: &impl LoggerTrait) {
    logger.log(&Record {
        level: Level::Info,
        target: "test_target",
        arguments: format_args!("This is a test log message."),
    });
    logger.log(&Record {
        level: Level::Warn,
        target: "test_target",
        arguments: format_args!("This is a test warning message."),
    });
    logger.log(&Record {
        level: Level::Error,
        target: "test_target",
        arguments: format_args!("This is a test error message."),
    });
    logger.log(&Record {
        level: Level::Debug,
        target: "test_target",
        arguments: format_args!("This is a test debug message."),
    });
    logger.log(&Record {
        level: Level::Trace,
        target: "test_target",
        arguments: format_args!("This is a test trace message."),
    });
}

pub fn test_flush(logger: &impl LoggerTrait) {
    logger.flush();
}
