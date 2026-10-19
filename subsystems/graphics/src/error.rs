use core::fmt::Display;
use embedded_i18n::translate;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    InvalidReference,
    AlreadyInitialized,
    NotInitialized,
    OutOfMemory,
    AlreadyInUse,
    FailedToRegister,
    FailedToGetResolution,
    NotRegistered,
    NotAvailable,
    FailedToCreateObject,
    InvalidWindowIdentifier,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        let string = match self {
            Error::InvalidReference => translate!("Invalid reference"),
            Error::AlreadyInitialized => translate!("Already initialized"),
            Error::NotInitialized => translate!("Not initialized"),
            Error::OutOfMemory => translate!("Out of memory"),
            Error::AlreadyInUse => translate!("Already in use"),
            Error::FailedToRegister => translate!("Failed to register"),
            Error::FailedToGetResolution => translate!("Failed to get resolution"),
            Error::NotRegistered => translate!("Not registered"),
            Error::NotAvailable => translate!("Not available"),
            Error::FailedToCreateObject => translate!("Failed to create object"),
            Error::InvalidWindowIdentifier => translate!("Invalid window identifier"),
        };

        write!(formatter, "{string}")
    }
}
