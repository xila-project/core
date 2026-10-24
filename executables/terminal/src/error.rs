use core::num::NonZeroUsize;
use core::str::Utf8Error;
use core::{fmt::Display, num::NonZeroU8};
use xila::{executable, graphics, internationalization::translate, task, virtual_file_system};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone)]
#[repr(u8)]
pub enum Error {
    Graphics(graphics::Error) = 1,
    FailedToCreateObject,
    Utf8(Utf8Error),
    FailedToMountDevice(virtual_file_system::Error),
    FailedToGetTaskIdentifier(task::Error),
    FailedToExecute(executable::Error),
    RessourceBusy,
}

impl Error {
    pub fn get_discriminant(&self) -> NonZeroU8 {
        unsafe { *(self as *const Self as *const NonZeroU8) }
    }
}

impl From<executable::Error> for Error {
    fn from(error: executable::Error) -> Self {
        Self::FailedToExecute(error)
    }
}

impl From<task::Error> for Error {
    fn from(error: task::Error) -> Self {
        Self::FailedToGetTaskIdentifier(error)
    }
}

impl From<virtual_file_system::Error> for Error {
    fn from(error: virtual_file_system::Error) -> Self {
        Self::FailedToMountDevice(error)
    }
}

impl From<Utf8Error> for Error {
    fn from(error: Utf8Error) -> Self {
        Self::Utf8(error)
    }
}

impl From<graphics::Error> for Error {
    fn from(error: graphics::Error) -> Self {
        Self::Graphics(error)
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::Graphics(error) => {
                write!(formatter, translate!("errors.graphics"), error)
            }
            Self::FailedToCreateObject => {
                write!(formatter, translate!("errors.failed_to_create_object"))
            }
            Self::Utf8(error) => {
                write!(formatter, translate!("errors.utf_8"), error)
            }
            Self::FailedToMountDevice(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_mount_device"),
                    error
                )
            }
            Self::FailedToGetTaskIdentifier(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_get_task_identifier"),
                    error
                )
            }
            Self::FailedToExecute(error) => {
                write!(formatter, translate!("errors.failed_to_execute"), error)
            }
            Self::RessourceBusy => {
                write!(formatter, translate!("errors.resource_busy"))
            }
        }
    }
}

impl From<Error> for NonZeroUsize {
    fn from(error: Error) -> Self {
        error.get_discriminant().into()
    }
}
