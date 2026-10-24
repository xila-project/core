use core::{
    convert::Infallible,
    fmt::Display,
    num::{NonZeroU8, NonZeroUsize},
};
use embedded_io::WriteFmtError;
use xila::{
    authentication, file_system, graphics, internationalization::translate, task,
    virtual_file_system,
};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum Error {
    Graphics(graphics::Error) = 1,
    VirtualFileSystem(virtual_file_system::Error),
    FailedToCreateObject,
    FailedToGetChild,
    FailedToSetEnvironmentVariable(task::Error),
    InvalidUtf8(core::str::Utf8Error),
    FailedToSetTaskUser(task::Error),
    FailedToGetCurrentTaskIdentifier(task::Error),
    FailedToReadDirectory(file_system::Error),
    FailedToOpenStandardFile(file_system::Error),
    NullCharacterInString(alloc::ffi::NulError),
    FailedToCreateUiElement,
    Authentication(authentication::Error),
    FormattingError,
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

impl From<virtual_file_system::Error> for Error {
    fn from(error: virtual_file_system::Error) -> Self {
        Error::VirtualFileSystem(error)
    }
}

impl From<task::Error> for Error {
    fn from(error: task::Error) -> Self {
        Error::FailedToSetEnvironmentVariable(error)
    }
}

impl From<core::str::Utf8Error> for Error {
    fn from(error: core::str::Utf8Error) -> Self {
        Error::InvalidUtf8(error)
    }
}

impl From<alloc::ffi::NulError> for Error {
    fn from(error: alloc::ffi::NulError) -> Self {
        Error::NullCharacterInString(error)
    }
}

impl From<core::fmt::Error> for Error {
    fn from(_: core::fmt::Error) -> Self {
        Error::FormattingError
    }
}

impl From<WriteFmtError<Infallible>> for Error {
    fn from(_: WriteFmtError<Infallible>) -> Self {
        Error::FormattingError
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::Graphics(error) => {
                write!(formatter, translate!("errors.graphics_error"), error)
            }
            Error::VirtualFileSystem(error) => {
                write!(
                    formatter,
                    translate!("errors.virtual_file_system_error"),
                    error
                )
            }
            Error::FailedToCreateObject => {
                write!(formatter, translate!("errors.failed_to_create_object"))
            }
            Error::FailedToGetChild => {
                write!(formatter, translate!("errors.failed_to_get_child_object"))
            }
            Error::FailedToSetEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_set_environment_variable"),
                    error
                )
            }
            Error::InvalidUtf8(error) => {
                write!(formatter, translate!("errors.invalid_utf_8_string"), error)
            }
            Error::FailedToSetTaskUser(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_set_task_user"),
                    error
                )
            }
            Error::FailedToGetCurrentTaskIdentifier(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_get_current_task_identifier"),
                    error
                )
            }
            Error::FailedToReadDirectory(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_read_directory"),
                    error
                )
            }
            Error::FailedToOpenStandardFile(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_open_standard_file"),
                    error
                )
            }
            Error::NullCharacterInString(error) => {
                write!(
                    formatter,
                    translate!("errors.null_character_in_string"),
                    error
                )
            }
            Error::FailedToCreateUiElement => {
                write!(formatter, translate!("errors.failed_to_create_ui_element"))
            }
            Error::Authentication(error) => {
                write!(formatter, translate!("errors.authentication_failed"), error)
            }
            Error::FormattingError => {
                write!(formatter, translate!("errors.formatting_error"))
            }
        }
    }
}
