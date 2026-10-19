use core::fmt::Display;
use embedded_i18n::translate;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NotInitialized,
    AlreadyInitialized,
    DeviceError(file_system::Error),
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotInitialized => write!(f, translate!("Time module not initialized")),
            Error::AlreadyInitialized => {
                write!(f, translate!("Time module already initialized"))
            }
            Error::DeviceError(e) => write!(f, translate!("Device error: {}"), e),
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;
