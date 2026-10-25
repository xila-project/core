use core::{fmt::Display, num::NonZeroU32};

use embedded_i18n::translate;
use embedded_io_async::ErrorKind;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, PartialEq, Clone, Copy, Eq)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    UnavailableDriver,
    InvalidFileSystem,
    InvalidParameter,
    TooManyOpenFiles,
    FailedToGetTaskInformations,
    InvalidIdentifier,
    AlreadyExists,
    Time(time::Error),
    FileSystem(file_system::Error) = 0x100,
    Users(users::Error) = 0x300,
    Task(task::Error) = 0x400,
    MissingAttribute,
    InvalidPath,
    PermissionDenied,
    TooManyInodes,
    RessourceBusy,
    NotADirectory,
    NotAFile,
    InvalidInode,
    InvalidMode,
    InvalidOpen,
    UnsupportedOperation,
    FailedToWrite,
    DelimiterNotFound,
    Orphaned,
}

impl Error {
    pub fn get_discriminant(&self) -> NonZeroU32 {
        unsafe { *<*const _>::from(self).cast::<NonZeroU32>() }
    }
}

impl From<Error> for u32 {
    fn from(value: Error) -> Self {
        NonZeroU32::from(value).get()
    }
}

impl From<Error> for NonZeroU32 {
    fn from(value: Error) -> Self {
        let discriminant = value.get_discriminant();

        let offset = match value {
            Error::FileSystem(error_type) => error_type.get_discriminant().get(),
            _ => 0,
        };

        discriminant.saturating_add(offset)
    }
}

impl From<users::Error> for Error {
    fn from(value: users::Error) -> Self {
        Self::Users(value)
    }
}

impl From<time::Error> for Error {
    fn from(value: time::Error) -> Self {
        Self::Time(value)
    }
}

impl From<file_system::Error> for Error {
    fn from(value: file_system::Error) -> Self {
        Self::FileSystem(value)
    }
}

impl From<task::Error> for Error {
    fn from(value: task::Error) -> Self {
        Self::Task(value)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::AlreadyInitialized => write!(f, translate!("errors.already_initialized")),
            Error::UnavailableDriver => write!(f, translate!("errors.unavailable_driver")),
            Error::InvalidFileSystem => write!(f, translate!("errors.invalid_file_system")),
            Error::InvalidParameter => write!(f, translate!("errors.invalid_parameter")),
            Error::TooManyOpenFiles => write!(f, translate!("errors.too_many_open_files")),
            Error::FailedToGetTaskInformations => {
                write!(f, translate!("errors.failed_to_get_task_informations"))
            }
            Error::FileSystem(err) => write!(f, translate!("errors.file_system_error"), err),
            Error::InvalidIdentifier => write!(f, translate!("errors.invalid_identifier")),
            Error::AlreadyExists => write!(f, translate!("errors.already_exists")),
            Error::Time(err) => write!(f, translate!("errors.time_error"), err),
            Error::Users(err) => write!(f, translate!("errors.users_error"), err),
            Error::Task(err) => write!(f, translate!("errors.task_error"), err),
            Error::MissingAttribute => write!(f, translate!("errors.missing_attribute")),
            Error::InvalidPath => write!(f, translate!("errors.invalid_path")),
            Error::PermissionDenied => write!(f, translate!("errors.permission_denied")),
            Error::TooManyInodes => write!(f, translate!("errors.too_many_inodes")),
            Error::RessourceBusy => write!(f, translate!("errors.ressource_busy")),
            Error::NotADirectory => write!(f, translate!("errors.not_a_directory")),
            Error::NotAFile => write!(f, translate!("errors.not_a_file")),
            Error::InvalidMode => write!(f, translate!("errors.invalid_mode")),
            Error::InvalidOpen => write!(f, translate!("errors.invalid_open")),
            Error::InvalidInode => write!(f, translate!("errors.invalid_inode")),
            Error::UnsupportedOperation => write!(f, translate!("errors.unsupported_operation")),
            Error::FailedToWrite => write!(f, translate!("errors.failed_to_write")),
            Error::DelimiterNotFound => write!(f, translate!("errors.delimiter_not_found")),
            Error::Orphaned => write!(f, translate!("errors.orphaned")),
        }
    }
}

impl core::error::Error for Error {}

impl embedded_io_async::Error for Error {
    fn kind(&self) -> ErrorKind {
        match self {
            Error::PermissionDenied => ErrorKind::PermissionDenied,
            Error::NotADirectory => ErrorKind::InvalidInput,
            Error::NotAFile => ErrorKind::InvalidInput,
            Error::InvalidMode => ErrorKind::InvalidInput,
            Error::InvalidOpen => ErrorKind::InvalidInput,
            Error::UnsupportedOperation => ErrorKind::Unsupported,
            _ => ErrorKind::Other,
        }
    }
}
