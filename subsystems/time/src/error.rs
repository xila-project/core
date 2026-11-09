use core::fmt::Display;
use embedded_i18n::t;

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
            Error::Unsynchronized => write!(f, "{}", t!("errors.utc_unsynchronized")),
            Error::Overflow => write!(f, "{}", t!("errors.timestamp_overflow")),
            Error::InvalidResolution => write!(f, "{}", t!("errors.invalid_clock_resolution")),
            Error::NotInitialized => write!(f, "{}", t!("errors.time_module_not_initialized")),
            Error::AlreadyInitialized => {
                write!(f, "{}", t!("errors.time_module_already_initialized"))
            }
            Error::DeviceError(e) => write!(f, "{}", t!("errors.device_error", arg0 = e)),
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;
