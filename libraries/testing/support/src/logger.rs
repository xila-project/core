//! A logger that prints errors, warnings and information to standard output.

use core::fmt;
use log::LoggerTrait;

pub struct Logger;

impl LoggerTrait for Logger {
    fn enabled(&self, level: log::Level) -> bool {
        matches!(
            level,
            log::Level::Error | log::Level::Warn | log::Level::Info
        )
    }

    fn write(&self, arguments: fmt::Arguments) {
        std::println!("{arguments}");
    }
}
