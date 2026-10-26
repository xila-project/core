use core::fmt::Display;
use embedded_i18n::translate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unsynchronized,
    Overflow,
    InvalidResolution,
    NotInitialized,
    AlreadyInitialized,
    DeviceError(file_system::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Unsynchronized => write!(f, translate!("errors.utc_unsynchronized")),
            Error::Overflow => write!(f, translate!("errors.timestamp_overflow")),
            Error::InvalidResolution => write!(f, translate!("errors.invalid_clock_resolution")),
            Error::NotInitialized => write!(f, translate!("errors.time_module_not_initialized")),
            Error::AlreadyInitialized => {
                write!(f, translate!("errors.time_module_already_initialized"))
            }
            Error::DeviceError(e) => write!(f, translate!("errors.device_error"), e),
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;
