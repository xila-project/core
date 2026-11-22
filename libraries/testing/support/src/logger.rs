//! A logger that prints errors, warnings and information to standard output,
//! and checks that exercise any [`LoggerTrait`] implementation.

use core::fmt;
use log::{Level, LoggerTrait, Record};

pub struct Logger;

impl LoggerTrait for Logger {
    fn enabled(&self, level: Level) -> bool {
        matches!(level, Level::Error | Level::Warning | Level::Information)
    }

    fn write(&self, arguments: fmt::Arguments) {
        std::println!("{arguments}");
    }
}

/// Writes five numbered messages through `logger`.
pub fn test_write(logger: &impl LoggerTrait) {
    for number in 0..5 {
        logger.write(format_args!("Test message number {number}"));
    }
}

/// Logs one message at every level through `logger`.
pub fn test_log(logger: &impl LoggerTrait) {
    for level in Level::ALL {
        logger.log(&Record {
            level,
            target: "test_target",
            arguments: format_args!("Test {level} message"),
        });
    }
}

/// Flushes `logger`.
pub fn test_flush(logger: &impl LoggerTrait) {
    logger.flush();
}
