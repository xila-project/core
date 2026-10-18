use core::{
    fmt::Display,
    num::{NonZeroU8, NonZeroUsize},
};

pub type Result<T> = core::result::Result<T, Error>;

use xila::{
    authentication, executable, graphics, internationalization::translate, task,
    virtual_file_system,
};

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum Error {
    Graphics(graphics::Error) = 1,
    FailedToCreateObject,
    FailedToGetChild,
    FailedToSetEnvironmentVariable(task::Error),
    InvalidUtf8(core::str::Utf8Error),
    AuthenticationFailed(authentication::Error),
    FailedToSetTaskUser(task::Error),
    FailedToDeserializeShortcut(miniserde::Error),
    FailedToGetCurrentTaskIdentifier(task::Error),
    FailedToReadShortcutDirectory(virtual_file_system::Error),
    FailedToGetShortcutFilePath,
    FailedToReadShortcutFile(virtual_file_system::Error),
    FailedToOpenStandardFile(executable::Error),
    FailedToExecuteShortcut(executable::Error),
    NullCharacterInString(alloc::ffi::NulError),
    MissingArguments,
    InvalidOption,
    InvalidNumberOfArguments,
    FailedToAddShortcut(virtual_file_system::Error),
    FailedToOpenDirectory(virtual_file_system::Error),
}

impl Error {
    pub fn get_discriminant(&self) -> NonZeroU8 {
        unsafe { *(self as *const Self as *const NonZeroU8) }
    }
}

impl From<Error> for NonZeroUsize {
    fn from(error: Error) -> Self {
        error.get_discriminant().into()
    }
}

impl From<graphics::Error> for Error {
    fn from(error: graphics::Error) -> Self {
        Error::Graphics(error)
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Graphics(error) => {
                write!(formatter, translate!("Graphics error: {}"), error)
            }
            Self::FailedToCreateObject => {
                write!(formatter, translate!("Failed to create object"))
            }
            Self::FailedToGetChild => {
                write!(formatter, translate!("Failed to get child"))
            }
            Self::FailedToSetEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("Failed to set environment variable: {}"),
                    error
                )
            }
            Self::InvalidUtf8(error) => {
                write!(formatter, translate!("Invalid UTF-8: {}"), error)
            }
            Self::AuthenticationFailed(error) => {
                write!(formatter, translate!("Authentication failed: {}"), error)
            }
            Self::FailedToSetTaskUser(error) => {
                write!(formatter, translate!("Failed to set task user: {}"), error)
            }
            Self::FailedToDeserializeShortcut(error) => {
                write!(
                    formatter,
                    translate!("Failed to deserialize shortcut: {}"),
                    error
                )
            }
            Self::FailedToGetCurrentTaskIdentifier(error) => {
                write!(
                    formatter,
                    translate!("Failed to get current task identifier: {}"),
                    error
                )
            }
            Self::FailedToReadShortcutDirectory(error) => {
                write!(
                    formatter,
                    translate!("Failed to read shortcut directory: {}"),
                    error
                )
            }
            Self::FailedToGetShortcutFilePath => {
                write!(formatter, translate!("Failed to get shortcut file path"))
            }
            Self::FailedToReadShortcutFile(error) => {
                write!(
                    formatter,
                    translate!("Failed to read shortcut file: {}"),
                    error
                )
            }
            Self::FailedToOpenStandardFile(error) => {
                write!(
                    formatter,
                    translate!("Failed to open standard file: {}"),
                    error
                )
            }
            Self::FailedToExecuteShortcut(error) => {
                write!(
                    formatter,
                    translate!("Failed to execute shortcut: {}"),
                    error
                )
            }
            Self::NullCharacterInString(error) => {
                write!(formatter, translate!("Null character in string: {}"), error)
            }
            Self::MissingArguments => {
                write!(formatter, translate!("Missing arguments"))
            }
            Self::InvalidOption => {
                write!(formatter, "Invalid option")
            }
            Self::InvalidNumberOfArguments => {
                write!(formatter, "Invalid number of arguments")
            }
            Self::FailedToAddShortcut(error) => {
                write!(formatter, translate!("Failed to add shortcut: {}"), error)
            }
            Self::FailedToOpenDirectory(error) => {
                write!(formatter, translate!("Failed to open directory: {}"), error)
            }
        }
    }
}
