//! The logger checks run against the test logger and a capturing logger.

use log::{Level, LoggerTrait};
use std::fmt;
use std::sync::Mutex;
use testing_support::Logger;
use testing_support::logger::{test_flush, test_log, test_write};

#[test]
fn test_logger_keeps_errors_warnings_and_information() {
    assert!(Logger.enabled(Level::Error));
    assert!(Logger.enabled(Level::Warning));
    assert!(Logger.enabled(Level::Information));
    assert!(!Logger.enabled(Level::Debug));
    assert!(!Logger.enabled(Level::Trace));

    test_write(&Logger);
    test_log(&Logger);
    test_flush(&Logger);
}

struct Capture(Mutex<Vec<String>>);

impl LoggerTrait for Capture {
    fn enabled(&self, _: Level) -> bool {
        true
    }

    fn write(&self, arguments: fmt::Arguments) {
        self.0.lock().unwrap().push(arguments.to_string());
    }
}

#[test]
fn checks_reach_the_logger() {
    let capture = Capture(Mutex::new(Vec::new()));

    test_write(&capture);
    test_log(&capture);
    test_flush(&capture);

    let messages = capture.0.into_inner().unwrap();
    assert_eq!(messages.len(), 10);
    assert_eq!(messages[0], "Test message number 0");
    assert!(
        messages[5].ends_with("| Test error message"),
        "{}",
        messages[5]
    );
    assert!(
        messages[6].contains("| Test warning message"),
        "{}",
        messages[6]
    );
    assert!(messages[7].contains("test_target"), "{}", messages[7]);
    assert!(
        messages[7].ends_with("| Test information message"),
        "{}",
        messages[7]
    );
    assert!(
        messages[9].ends_with("| Test trace message"),
        "{}",
        messages[9]
    );
}
