use core::{fmt, num::NonZeroU32};
use embedded_i18n::translate;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    InvalidTaskIdentifier = 1,
    InvalidSpawnerIdentifier,
    ThreadNotRegistered,
    ThreadAlreadyRegistered,
    FailedToCreateThread,
    NoThreadForTask,
    FailedToSpawnThread,
    InvalidEnvironmentVariable,
    TooManyTasks,
    TooManySpawners,
    AlreadyInitialized,
    AlreadySet,
    NotInitialized,
    NoSpawnerAvailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::InvalidTaskIdentifier => write!(f, translate!("InvalidTaskIdentifier")),
            Error::InvalidSpawnerIdentifier => write!(f, translate!("InvalidSpawnerIdentifier")),
            Error::ThreadNotRegistered => write!(f, translate!("ThreadNotRegistered")),
            Error::ThreadAlreadyRegistered => write!(f, translate!("ThreadAlreadyRegistered")),
            Error::FailedToCreateThread => write!(f, translate!("FailedToCreateThread")),
            Error::NoThreadForTask => write!(f, translate!("NoThreadForTask")),
            Error::FailedToSpawnThread => write!(f, translate!("FailedToSpawnThread")),
            Error::InvalidEnvironmentVariable => {
                write!(f, translate!("InvalidEnvironmentVariable"))
            }
            Error::TooManyTasks => write!(f, translate!("TooManyTasks")),
            Error::TooManySpawners => write!(f, translate!("TooManySpawners")),
            Error::AlreadyInitialized => write!(f, translate!("AlreadyInitialized")),
            Error::AlreadySet => write!(f, translate!("AlreadySet")),
            Error::NotInitialized => write!(f, translate!("NotInitialized")),
            Error::NoSpawnerAvailable => write!(f, translate!("NoSpawnerAvailable")),
        }
    }
}

impl core::error::Error for Error {}

impl From<Error> for NonZeroU32 {
    fn from(error: Error) -> Self {
        unsafe { NonZeroU32::new_unchecked(error as u32) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::{String, ToString};
    use std::vec;

    #[test]
    fn test_error_type_display() {
        let error = Error::InvalidTaskIdentifier;
        let display_string = format!("{error}");
        assert_eq!(display_string, "Invalid task identifier");

        let error = Error::TooManyTasks;
        let display_string = format!("{error}");
        assert_eq!(display_string, "Too many tasks");
    }

    #[test]
    fn test_error_type_debug() {
        let error = Error::InvalidSpawnerIdentifier;
        let debug_string = format!("{error:?}");
        assert_eq!(debug_string, "InvalidSpawnerIdentifier");

        let error = Error::ThreadNotRegistered;
        let debug_string = format!("{error:?}");
        assert_eq!(debug_string, "ThreadNotRegistered");
    }

    #[test]
    fn test_error_type_clone() {
        let error1 = Error::FailedToCreateThread;
        let error2 = error1.clone();

        assert_eq!(format!("{error1:?}"), format!("{:?}", error2));
    }

    #[test]
    fn test_all_error_variants() {
        let errors = vec![
            Error::InvalidTaskIdentifier,
            Error::InvalidSpawnerIdentifier,
            Error::ThreadNotRegistered,
            Error::ThreadAlreadyRegistered,
            Error::FailedToCreateThread,
            Error::NoThreadForTask,
            Error::FailedToSpawnThread,
            Error::InvalidEnvironmentVariable,
            Error::TooManyTasks,
            Error::TooManySpawners,
            Error::AlreadyInitialized,
            Error::AlreadySet,
            Error::NotInitialized,
            Error::NoSpawnerAvailable,
        ];

        // Test that all variants can be created and formatted
        for error in errors {
            let debug_str = format!("{error:?}");
            let display_str = format!("{error}");

            assert!(!debug_str.is_empty());
            assert!(!display_str.is_empty());
        }
    }

    #[test]
    fn test_error_to_nonzero_u32_conversion() {
        let errors_and_expected_values = vec![
            (Error::InvalidTaskIdentifier, 1u32),
            (Error::InvalidSpawnerIdentifier, 2u32),
            (Error::ThreadNotRegistered, 3u32),
            (Error::ThreadAlreadyRegistered, 4u32),
            (Error::FailedToCreateThread, 5u32),
            (Error::NoThreadForTask, 6u32),
            (Error::FailedToSpawnThread, 7u32),
            (Error::InvalidEnvironmentVariable, 8u32),
            (Error::TooManyTasks, 9u32),
            (Error::TooManySpawners, 10u32),
            (Error::AlreadyInitialized, 11u32),
            (Error::AlreadySet, 12u32),
            (Error::NotInitialized, 13u32),
            (Error::NoSpawnerAvailable, 14u32),
        ];

        for (error, expected_value) in errors_and_expected_values {
            let non_zero: NonZeroU32 = error.into();
            assert_eq!(non_zero.get(), expected_value);
        }
    }

    #[test]
    fn test_result_type_ok() {
        let result: Result<i32> = Ok(42);
        assert!(result.is_ok());
        if let Ok(value) = result {
            assert_eq!(value, 42);
        }
    }

    #[test]
    fn test_result_type_err() {
        let result: Result<i32> = Err(Error::InvalidTaskIdentifier);
        assert!(result.is_err());

        if let Err(error) = result {
            assert_eq!(format!("{error:?}"), "InvalidTaskIdentifier");
        }
    }

    #[test]
    fn test_error_in_result_chain() {
        fn might_fail(should_fail: bool) -> Result<String> {
            if should_fail {
                Err(Error::TooManyTasks)
            } else {
                Ok("Success".to_string())
            }
        }

        let success_result = might_fail(false);
        assert!(success_result.is_ok());
        if let Ok(value) = success_result {
            assert_eq!(value, "Success");
        }

        let failure_result = might_fail(true);
        assert!(failure_result.is_err());

        if let Err(error) = failure_result {
            assert_eq!(format!("{error:?}"), "TooManyTasks");
        }
    }

    #[test]
    fn test_error_propagation() {
        fn inner_function() -> Result<i32> {
            Err(Error::NotInitialized)
        }

        fn outer_function() -> Result<String> {
            let _value = inner_function()?;
            Ok("This won't be reached".to_string())
        }

        let result = outer_function();
        assert!(result.is_err());

        if let Err(error) = result {
            assert_eq!(format!("{error:?}"), "NotInitialized");
        }
    }

    #[test]
    fn test_error_size() {
        // Ensure the error type has a reasonable size
        assert!(std::mem::size_of::<Error>() <= 8);
    }

    #[test]
    fn test_specific_error_scenarios() {
        // Test environment variable error specifically
        let env_error = Error::InvalidEnvironmentVariable;
        let display_str = format!("{env_error}");
        let non_zero: NonZeroU32 = env_error.into();
        assert_eq!(non_zero.get(), 8u32);
        assert_eq!(display_str, "Invalid environment variable");

        // Test task-related errors
        let task_error = Error::InvalidTaskIdentifier;
        assert_eq!(format!("{task_error}"), "Invalid task identifier");

        let spawner_error = Error::InvalidSpawnerIdentifier;
        assert_eq!(format!("{spawner_error}"), "Invalid spawner identifier");
    }
}
