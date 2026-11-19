use core::fmt::Display;
use embedded_i18n::t;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub enum Error {
    FileSystem(virtual_file_system::Error),
    Task(task::Error),
    FailedToGetMainFunction,
    InvalidStackSize,
    PermissionDenied,
    #[cfg(feature = "native-dev")]
    Native(native_loader::Error),
    #[cfg(feature = "native-dev")]
    NativeFileTooLarge,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::FileSystem(error) => write!(formatter, "{error}"),
            Error::Task(error) => write!(formatter, "{error}"),
            Error::FailedToGetMainFunction => {
                write!(formatter, "{}", t!("errors.failed_to_get_main_function"))
            }
            Error::InvalidStackSize => write!(formatter, "{}", t!("errors.invalid_stack_size")),
            #[cfg(feature = "native-dev")]
            Error::Native(error) => write!(formatter, "{} ({error:?})", t!("errors.native_load")),
            #[cfg(feature = "native-dev")]
            Error::NativeFileTooLarge => {
                write!(formatter, "{}", t!("errors.native_file_too_large"))
            }
            Error::PermissionDenied => write!(formatter, "{}", t!("errors.permission_denied")),
        }
    }
}

impl From<virtual_file_system::Error> for Error {
    fn from(error: virtual_file_system::Error) -> Self {
        Error::FileSystem(error)
    }
}

impl From<task::Error> for Error {
    fn from(error: task::Error) -> Self {
        Error::Task(error)
    }
}
