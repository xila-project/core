//! Error handling for the Authentication module.
//!
//! This module defines all possible errors that can occur during authentication operations,
//! including user and group management, file I/O, and password validation.

use core::fmt::Display;
use embedded_i18n::t;

/// Result type alias for authentication operations.
///
/// This is a convenience type that wraps `Result<T, Error>` for all
/// authentication-related operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Comprehensive error enumeration for authentication operations.
///
/// This enum covers all possible error conditions that can occur during:
/// - User authentication and management
/// - Group management
/// - File system operations
/// - Password hashing and validation
/// - Random salt generation
#[derive(Debug, Clone)]
pub enum Error {
    /// Failed to get the current task identifier
    FailedToGetCurrentTaskIdentifier(task::Error),
    /// Failed to read the users directory from the filesystem
    FailedToReadUsersDirectory(virtual_file_system::Error),
    /// Failed to construct a valid user file path
    FailedToGetUserFilePath,
    /// Failed to open a user file for reading or writing
    FailedToOpenUserFile(virtual_file_system::Error),
    /// Failed to read the contents of a user file
    FailedToReadUserFile(virtual_file_system::Error),
    /// Failed to parse JSON content from a user file
    FailedToParseUserFile(miniserde::Error),
    /// Failed to add a user to the Users manager
    FailedToAddUser(users::Error),
    /// Failed to generate a new unique user identifier
    FailedToGetNewUserIdentifier(users::Error),
    /// Failed to create a new user account
    FailedToCreateUser(users::Error),
    /// Failed to write user data to a file
    FailedToWriteUserFile(virtual_file_system::Error),
    /// Failed to create the users directory
    FailedToCreateUsersDirectory(virtual_file_system::Error),
    /// Failed to read the groups directory from the filesystem
    FailedToReadGroupDirectory(virtual_file_system::Error),
    /// Failed to construct a valid group file path
    FailedToGetGroupFilePath,
    /// Failed to open a group file for reading or writing
    FailedToOpenGroupFile(virtual_file_system::Error),
    /// Failed to read the contents of a group file
    FailedToReadGroupFile(virtual_file_system::Error),
    /// Failed to parse JSON content from a group file
    FailedToParseGroupFile(miniserde::Error),
    /// Failed to add a group to the Users manager
    FailedToAddGroup(users::Error),
    /// Failed to generate a new unique group identifier
    FailedToGetNewGroupIdentifier(users::Error),
    /// Failed to create a new group
    FailedToCreateGroup(users::Error),
    /// Failed to write group data to a file
    FailedToWriteGroupFile(virtual_file_system::Error),
    /// Failed to create the groups directory
    FailedToCreateGroupsDirectory(virtual_file_system::Error),
    /// The provided password is invalid or incorrect
    InvalidPassword,
    /// Failed to open the random device for salt generation
    FailedToOpenRandomDevice(virtual_file_system::Error),
    /// Failed to read random data from the random device
    FailedToReadRandomDevice(virtual_file_system::Error),
    /// Failed to get user identifier from the Users manager
    FailedToGetUserIdentifier(users::Error),
    /// Failed to close a file
    FailedToCloseFile(virtual_file_system::Error),
    /// Failed to hash
    FailedToHashPassword(virtual_file_system::Error),
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::FailedToGetCurrentTaskIdentifier(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_get_current_task_identifier", arg0 = error)
                )
            }
            Self::FailedToReadUsersDirectory(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_read_users_directory", arg0 = error)
                )
            }
            Self::FailedToGetUserFilePath => {
                write!(formatter, "{}", t!("errors.failed_to_get_user_file_path"))
            }
            Self::FailedToReadUserFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_read_user_file", arg0 = error)
                )
            }
            Self::FailedToOpenUserFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_open_user_file", arg0 = error)
                )
            }
            Self::FailedToParseUserFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_parse_user_file", arg0 = error)
                )
            }
            Self::FailedToAddUser(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_add_user", arg0 = error)
                )
            }
            Self::FailedToCreateUsersDirectory(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_create_users_directory", arg0 = error)
                )
            }
            Self::FailedToReadGroupDirectory(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_read_group_directory", arg0 = error)
                )
            }
            Self::FailedToGetGroupFilePath => {
                write!(formatter, "{}", t!("errors.failed_to_get_group_file_path"))
            }
            Self::FailedToOpenGroupFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_open_group_file", arg0 = error)
                )
            }
            Self::FailedToReadGroupFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_read_group_file", arg0 = error)
                )
            }
            Self::FailedToParseGroupFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_parse_group_file", arg0 = error)
                )
            }
            Self::FailedToAddGroup(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_add_group", arg0 = error)
                )
            }
            Self::FailedToCreateGroupsDirectory(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_create_group_directory", arg0 = error)
                )
            }
            Self::InvalidPassword => {
                write!(formatter, "{}", t!("errors.invalid_password"))
            }
            Self::FailedToOpenRandomDevice(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_open_random_device", arg0 = error)
                )
            }
            Self::FailedToReadRandomDevice(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_read_random_device", arg0 = error)
                )
            }
            Self::FailedToCreateUser(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_create_user", arg0 = error)
                )
            }
            Self::FailedToGetNewUserIdentifier(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_get_new_user_identifier", arg0 = error)
                )
            }
            Self::FailedToWriteUserFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_write_user_file", arg0 = error)
                )
            }
            Self::FailedToGetNewGroupIdentifier(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_get_new_groupe_identifier", arg0 = error)
                )
            }
            Self::FailedToCreateGroup(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_create_group", arg0 = error)
                )
            }
            Self::FailedToWriteGroupFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_write_group_file", arg0 = error)
                )
            }
            Self::FailedToGetUserIdentifier(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_get_user_identifier", arg0 = error)
                )
            }
            Self::FailedToCloseFile(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_close_file", arg0 = error)
                )
            }
            Self::FailedToHashPassword(error) => {
                write!(
                    formatter,
                    "{}",
                    t!("errors.failed_to_hash_password", arg0 = error)
                )
            }
        }
    }
}

impl From<task::Error> for Error {
    fn from(error: task::Error) -> Self {
        Self::FailedToGetCurrentTaskIdentifier(error)
    }
}
