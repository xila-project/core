//! Error types and result handling for file system operations.
//!
//! This module defines all possible errors that can occur during file system operations,
//! along with conversion traits and display implementations for comprehensive error reporting.

use core::{fmt::Display, num::NonZeroU32};
use embedded_i18n::translate;

/// Standard result type for file system operations.
///
/// This is a convenience alias for `Result<T, Error>` used throughout the file system crate.
/// All file system operations that can fail return this type.
///
/// # Examples
///
/// ```rust
/// # extern crate alloc;
/// use file_system::{Result, Error};
///
/// fn example_operation() -> Result<String> {
///     Ok("Success".into())
/// }
///
/// fn failing_operation() -> Result<()> {
///     Err(Error::PermissionDenied)
/// }
/// ```
pub type Result<T> = core::result::Result<T, Error>;

/// Comprehensive enumeration of all possible file system errors.
///
/// This enum covers errors that can occur at various levels of the file system stack,
/// from low-level device operations to high-level file system operations. Each variant
/// has a unique numeric discriminant for FFI compatibility.
#[derive(Debug, PartialEq, Clone, Copy, Eq)]
#[repr(C)]
pub enum Error {
    /// Failed to initialize the file system.
    FailedToInitializeFileSystem = 1,
    /// Permission denied for the requested operation.
    PermissionDenied,
    /// File, directory, or resource not found.
    NotFound,
    /// File or directory already exists.
    AlreadyExists,
    /// Directory already exists (more specific than Already_exists).
    DirectoryAlreadyExists,
    /// File system is full and cannot store more data.
    FileSystemFull,
    /// Generic file system error.
    FileSystemError,
    /// The provided path is invalid or malformed.
    InvalidPath,
    /// The file is corrupted or invalid.
    InvalidFile,
    /// The directory is corrupted or invalid.
    InvalidDirectory,
    /// The symbolic link is invalid or broken.
    InvalidSymbolicLink,
    /// Unknown or unspecified error.
    Unknown,
    /// File or task identifier is invalid.
    InvalidIdentifier,
    /// Failed to retrieve task information from task manager.
    FailedToGetTaskInformations,
    /// Failed to retrieve user information from user manager.
    FailedToGetUsersInformations,
    /// Maximum number of mounted file systems exceeded.
    TooManyMountedFileSystems,
    /// Maximum number of open files exceeded.
    TooManyOpenFiles,
    /// Internal implementation error.
    InternalError,
    /// Invalid access mode specified.
    InvalidMode,
    /// Operation is not supported by the device or file system.
    UnsupportedOperation,
    /// Resource is temporarily busy or unavailable.
    RessourceBusy,
    /// System or component is already initialized.
    AlreadyInitialized,
    /// System or component is not initialized.
    NotInitialized,
    /// Failed to get users manager instance.
    FailedToGetUsersManagerInstance,
    /// Failed to get task manager instance.
    FailedToGetTaskManagerInstance,
    /// Invalid parameter provided to function.
    InvalidParameter,
    /// Invalid flags specified for operation.
    InvalidFlags,
    /// Expected a directory but found a file.
    NotDirectory,
    /// Expected a file but found a directory.
    IsDirectory,
    /// Input/output error during operation.
    InputOutput,
    /// Directory is not empty and cannot be removed.
    DirectoryNotEmpty,
    /// File size exceeds maximum allowed size.
    FileTooLarge,
    /// Requested attribute does not exist.
    NoAttribute,
    /// File or directory name is too long.
    NameTooLong,
    /// Data corruption detected.
    Corrupted,
    /// Insufficient memory for operation.
    NoMemory,
    /// No space left on device.
    NoSpaceLeft,
    /// Error in timestamp or time-related operation.
    TimeError,
    /// Invalid inode reference.
    InvalidInode,
    /// Not mounted.
    NotMounted,
    /// Already mounted.
    AlreadyMounted,
    /// Invalid context
    InvalidContext,
    /// Other unclassified error.
    Other,
}

impl core::error::Error for Error {}

impl Error {
    /// Get the numeric discriminant of the error as a non-zero u32.
    ///
    /// This is useful for FFI operations where errors need to be represented
    /// as numeric codes.
    ///
    /// # Returns
    ///
    /// A `NonZeroU32` containing the error's discriminant value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # extern crate alloc;
    /// use file_system::Error;
    ///
    /// let error = Error::PermissionDenied;
    /// let code = error.get_discriminant();
    /// assert_eq!(code.get(), 2); // Permission_denied has discriminant 2
    /// ```
    pub fn get_discriminant(&self) -> NonZeroU32 {
        unsafe { NonZeroU32::new_unchecked(*self as u32) }
    }
}

/// Convert Task module errors to file system errors.
///
/// This allows transparent handling of task-related errors in file system operations.
impl From<task::Error> for Error {
    fn from(_: task::Error) -> Self {
        Error::FailedToGetTaskInformations
    }
}

/// Convert Users module errors to file system errors.
///
/// This allows transparent handling of user-related errors in file system operations.
impl From<users::Error> for Error {
    fn from(_: users::Error) -> Self {
        Error::FailedToGetUsersInformations
    }
}

/// Convert file system errors to numeric discriminants.
///
/// This conversion is useful for FFI where errors need to be represented as numbers.
impl From<Error> for NonZeroU32 {
    fn from(error: Error) -> Self {
        error.get_discriminant()
    }
}

/// Display implementation for user-friendly error messages.
///
/// Provides human-readable descriptions of all error variants.
impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        let string = match self {
            Error::FailedToInitializeFileSystem => {
                translate!("errors.failed_to_initialize_file_system")
            }
            Error::PermissionDenied => translate!("errors.permission_denied"),
            Error::NotFound => translate!("errors.not_found"),
            Error::AlreadyExists => translate!("errors.already_exists"),
            Error::DirectoryAlreadyExists => translate!("errors.directory_already_exists"),
            Error::FileSystemFull => translate!("errors.file_system_full"),
            Error::FileSystemError => translate!("errors.file_system_error"),
            Error::InvalidPath => translate!("errors.invalid_path"),
            Error::InvalidFile => translate!("errors.invalid_file"),
            Error::InvalidDirectory => translate!("errors.invalid_directory"),
            Error::InvalidSymbolicLink => translate!("errors.invalid_symbolic_link"),
            Error::Unknown => translate!("errors.unknown"),
            Error::InvalidIdentifier => translate!("errors.invalid_identifier"),
            Error::FailedToGetTaskInformations => {
                translate!("errors.failed_to_get_task_informations")
            }
            Error::FailedToGetUsersInformations => {
                translate!("errors.failed_to_get_users_informations")
            }
            Error::TooManyMountedFileSystems => translate!("errors.too_many_mounted_file_systems"),
            Error::TooManyOpenFiles => translate!("errors.too_many_open_files"),
            Error::InternalError => translate!("errors.internal_error"),
            Error::InvalidMode => translate!("errors.invalid_mode"),
            Error::UnsupportedOperation => translate!("errors.unsupported_operation"),
            Error::RessourceBusy => translate!("errors.ressource_busy"),
            Error::AlreadyInitialized => translate!("errors.already_initialized"),
            Error::NotInitialized => translate!("errors.not_initialized"),
            Error::FailedToGetUsersManagerInstance => {
                translate!("errors.failed_to_get_users_manager_instance")
            }
            Error::FailedToGetTaskManagerInstance => {
                translate!("errors.failed_to_get_task_manager_instance")
            }
            Error::InvalidParameter => translate!("errors.invalid_parameter"),
            Error::InvalidFlags => translate!("errors.invalid_flags"),
            Error::NotDirectory => translate!("errors.not_directory"),
            Error::IsDirectory => translate!("errors.is_directory"),
            Error::InputOutput => translate!("errors.input_output"),
            Error::DirectoryNotEmpty => translate!("errors.directory_not_empty"),
            Error::FileTooLarge => translate!("errors.file_too_large"),
            Error::NoAttribute => translate!("errors.no_attribute"),
            Error::NameTooLong => translate!("errors.name_too_long"),
            Error::Corrupted => translate!("errors.corrupted"),
            Error::NoMemory => translate!("errors.no_memory"),
            Error::NoSpaceLeft => translate!("errors.no_space_left"),
            Error::TimeError => translate!("errors.time_error"),
            Error::InvalidInode => translate!("errors.invalid_inode"),
            Error::NotMounted => translate!("errors.not_mounted"),
            Error::AlreadyMounted => translate!("errors.already_mounted"),
            Error::InvalidContext => translate!("errors.invalid_context"),
            Error::Other => translate!("errors.other"),
        };

        write!(formatter, "{string}")
    }
}

impl embedded_io_async::Error for Error {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        match self {
            Error::PermissionDenied => embedded_io_async::ErrorKind::PermissionDenied,
            Error::NotFound => embedded_io_async::ErrorKind::NotFound,
            Error::AlreadyExists | Error::DirectoryAlreadyExists => {
                embedded_io_async::ErrorKind::AlreadyExists
            }
            Error::FileSystemFull | Error::NoSpaceLeft => embedded_io_async::ErrorKind::OutOfMemory,
            Error::InputOutput => embedded_io_async::ErrorKind::Interrupted,
            _ => embedded_io_async::ErrorKind::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_error_discriminants() {
        // Test that each error has a unique discriminant
        assert_eq!(
            Error::FailedToInitializeFileSystem.get_discriminant().get(),
            1
        );
        assert_eq!(Error::PermissionDenied.get_discriminant().get(), 2);
        assert_eq!(Error::NotFound.get_discriminant().get(), 3);
        assert_eq!(Error::AlreadyExists.get_discriminant().get(), 4);
        assert_eq!(Error::DirectoryAlreadyExists.get_discriminant().get(), 5);

        // Test a few more to ensure discriminants are sequential
        assert_eq!(Error::FileSystemFull.get_discriminant().get(), 6);
        assert_eq!(Error::FileSystemError.get_discriminant().get(), 7);
        assert_eq!(Error::InvalidPath.get_discriminant().get(), 8);
    }

    #[test]
    fn test_error_display() {
        // Test display formatting for all error types
        assert_eq!(
            format!("{}", Error::FailedToInitializeFileSystem),
            translate!("errors.failed_to_initialize_file_system")
        );
        assert_eq!(
            format!("{}", Error::PermissionDenied),
            translate!("errors.permission_denied")
        );
        assert_eq!(
            format!("{}", Error::NotFound),
            translate!("errors.not_found")
        );
        assert_eq!(
            format!("{}", Error::AlreadyExists),
            translate!("errors.already_exists")
        );
        assert_eq!(
            format!("{}", Error::DirectoryAlreadyExists),
            translate!("errors.directory_already_exists")
        );
        assert_eq!(
            format!("{}", Error::FileSystemFull),
            translate!("errors.file_system_full")
        );
        assert_eq!(
            format!("{}", Error::FileSystemError),
            translate!("errors.file_system_error")
        );
        assert_eq!(
            format!("{}", Error::InvalidPath),
            translate!("errors.invalid_path")
        );
        assert_eq!(
            format!("{}", Error::InvalidFile),
            translate!("errors.invalid_file")
        );
        assert_eq!(
            format!("{}", Error::InvalidDirectory),
            translate!("errors.invalid_directory")
        );
        assert_eq!(
            format!("{}", Error::InvalidSymbolicLink),
            translate!("errors.invalid_symbolic_link")
        );
        assert_eq!(format!("{}", Error::Unknown), translate!("errors.unknown"));
        assert_eq!(
            format!("{}", Error::InvalidIdentifier),
            translate!("errors.invalid_identifier")
        );
        assert_eq!(
            format!("{}", Error::FailedToGetTaskInformations),
            translate!("errors.failed_to_get_task_informations")
        );
        assert_eq!(
            format!("{}", Error::FailedToGetUsersInformations),
            translate!("errors.failed_to_get_users_informations")
        );
        assert_eq!(
            format!("{}", Error::TooManyMountedFileSystems),
            translate!("errors.too_many_mounted_file_systems")
        );
        assert_eq!(
            format!("{}", Error::TooManyOpenFiles),
            translate!("errors.too_many_open_files")
        );
        assert_eq!(
            format!("{}", Error::InternalError),
            translate!("errors.internal_error")
        );
        assert_eq!(
            format!("{}", Error::InvalidMode),
            translate!("errors.invalid_mode")
        );
        assert_eq!(
            format!("{}", Error::UnsupportedOperation),
            translate!("errors.unsupported_operation")
        );
        assert_eq!(
            format!("{}", Error::RessourceBusy),
            translate!("errors.ressource_busy")
        );
        assert_eq!(
            format!("{}", Error::AlreadyInitialized),
            translate!("errors.already_initialized")
        );
        assert_eq!(
            format!("{}", Error::NotInitialized),
            translate!("errors.not_initialized")
        );
        assert_eq!(
            format!("{}", Error::FailedToGetUsersManagerInstance),
            translate!("errors.failed_to_get_users_manager_instance")
        );
        assert_eq!(
            format!("{}", Error::FailedToGetTaskManagerInstance),
            translate!("errors.failed_to_get_task_manager_instance")
        );
        assert_eq!(
            format!("{}", Error::InvalidParameter),
            translate!("errors.invalid_parameter")
        );
        assert_eq!(
            format!("{}", Error::InvalidFlags),
            translate!("errors.invalid_flags")
        );
        assert_eq!(
            format!("{}", Error::NotDirectory),
            translate!("errors.not_directory")
        );
        assert_eq!(
            format!("{}", Error::IsDirectory),
            translate!("errors.is_directory")
        );
        assert_eq!(
            format!("{}", Error::InputOutput),
            translate!("errors.input_output")
        );
        assert_eq!(
            format!("{}", Error::DirectoryNotEmpty),
            translate!("errors.directory_not_empty")
        );
        assert_eq!(
            format!("{}", Error::FileTooLarge),
            translate!("errors.file_too_large")
        );
        assert_eq!(
            format!("{}", Error::NoAttribute),
            translate!("errors.no_attribute")
        );
        assert_eq!(
            format!("{}", Error::NameTooLong),
            translate!("errors.name_too_long")
        );
        assert_eq!(
            format!("{}", Error::Corrupted),
            translate!("errors.corrupted")
        );
        assert_eq!(
            format!("{}", Error::NoMemory),
            translate!("errors.no_memory")
        );
        assert_eq!(
            format!("{}", Error::NoSpaceLeft),
            translate!("errors.no_space_left")
        );
        assert_eq!(
            format!("{}", Error::TimeError),
            translate!("errors.time_error")
        );
        assert_eq!(
            format!("{}", Error::InvalidInode),
            translate!("errors.invalid_inode")
        );
        assert_eq!(format!("{}", Error::Other), translate!("errors.other"));
    }

    #[test]
    fn test_error_debug() {
        // Test debug formatting
        let error = Error::PermissionDenied;
        let debug_str = format!("{error:?}");
        assert_eq!(debug_str, "PermissionDenied");
    }

    #[test]
    fn test_error_equality() {
        // Test equality and cloning
        let error1 = Error::NotFound;
        let error2 = Error::NotFound;
        let error3 = Error::PermissionDenied;

        assert_eq!(error1, error2);
        assert_ne!(error1, error3);

        let cloned = error1;
        assert_eq!(error1, cloned);
    }

    #[test]
    fn test_error_conversions() {
        // Test conversion to NonZeroU32
        let error = Error::NotFound;
        let discriminant: NonZeroU32 = error.into();
        assert_eq!(discriminant.get(), 3);

        // Test explicit discriminant access
        assert_eq!(error.get_discriminant().get(), 3);
    }

    #[test]
    fn test_result_type() {
        // Test the Result alias
        let success: Result<i32> = Ok(42);
        let failure: Result<i32> = Err(Error::PermissionDenied);

        assert_eq!(success, Ok(42));

        assert_eq!(failure, Err(Error::PermissionDenied));
    }

    #[test]
    fn test_error_categories() {
        // Test that errors can be categorized by their discriminant ranges

        // Initialization errors (1-3 range roughly)
        assert!(matches!(
            Error::FailedToInitializeFileSystem.get_discriminant().get(),
            1
        ));
        assert!(matches!(
            Error::AlreadyInitialized.get_discriminant().get(),
            22
        ));
        assert!(matches!(Error::NotInitialized.get_discriminant().get(), 23));

        // Permission errors
        assert!(matches!(
            Error::PermissionDenied.get_discriminant().get(),
            2
        ));
        assert!(matches!(Error::InvalidMode.get_discriminant().get(), 19));

        // File/Directory errors
        assert!(matches!(Error::NotFound.get_discriminant().get(), 3));
        assert!(matches!(Error::AlreadyExists.get_discriminant().get(), 4));
        assert!(matches!(
            Error::DirectoryAlreadyExists.get_discriminant().get(),
            5
        ));
    }

    #[test]
    fn test_error_copy_semantics() {
        // Test that Error implements Copy
        let error = Error::FileSystemFull;
        let copied = error; // This should work due to Copy trait

        // Both should be usable
        assert_eq!(error, Error::FileSystemFull);
        assert_eq!(copied, Error::FileSystemFull);
        assert_eq!(error, copied);
    }

    #[test]
    fn test_error_size() {
        // Ensure Error has a reasonable size for an enum
        use core::mem::size_of;

        // Should be small since it's a C-style enum
        assert!(size_of::<Error>() <= 4); // Should be 4 bytes or less
    }

    #[test]
    fn test_nonzero_conversion() {
        // Test that all errors convert to valid NonZeroU32
        let errors = [
            Error::FailedToInitializeFileSystem,
            Error::PermissionDenied,
            Error::NotFound,
            Error::AlreadyExists,
            Error::DirectoryAlreadyExists,
            Error::FileSystemFull,
            Error::FileSystemError,
            Error::InvalidPath,
            Error::InvalidFile,
            Error::InvalidDirectory,
            Error::InvalidSymbolicLink,
            Error::Unknown,
            Error::InvalidIdentifier,
            Error::FailedToGetTaskInformations,
            Error::FailedToGetUsersInformations,
            Error::TooManyMountedFileSystems,
            Error::TooManyOpenFiles,
            Error::InternalError,
            Error::InvalidMode,
            Error::UnsupportedOperation,
            Error::RessourceBusy,
            Error::AlreadyInitialized,
            Error::NotInitialized,
            Error::FailedToGetUsersManagerInstance,
            Error::FailedToGetTaskManagerInstance,
            Error::InvalidParameter,
            Error::InvalidFlags,
            Error::NotDirectory,
            Error::IsDirectory,
            Error::InputOutput,
            Error::DirectoryNotEmpty,
            Error::FileTooLarge,
            Error::NoAttribute,
            Error::NameTooLong,
            Error::Corrupted,
            Error::NoMemory,
            Error::NoSpaceLeft,
            Error::TimeError,
            Error::InvalidInode,
            Error::Other,
        ];

        for error in errors.iter() {
            let discriminant = error.get_discriminant();
            assert!(discriminant.get() > 0);

            let converted: NonZeroU32 = (*error).into();
            assert_eq!(discriminant, converted);
        }
    }
}
