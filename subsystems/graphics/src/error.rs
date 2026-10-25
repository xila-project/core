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
    InvalidParameter,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        let string = match self {
            Error::InvalidReference => translate!("errors.invalid_reference"),
            Error::AlreadyInitialized => translate!("errors.already_initialized"),
            Error::NotInitialized => translate!("errors.not_initialized"),
            Error::OutOfMemory => translate!("errors.out_of_memory"),
            Error::AlreadyInUse => translate!("errors.already_in_use"),
            Error::FailedToRegister => translate!("errors.failed_to_register"),
            Error::FailedToGetResolution => translate!("errors.failed_to_get_resolution"),
            Error::NotRegistered => translate!("errors.not_registered"),
            Error::NotAvailable => translate!("errors.not_available"),
            Error::FailedToCreateObject => translate!("errors.failed_to_create_object"),
            Error::InvalidWindowIdentifier => translate!("errors.invalid_window_identifier"),
            Error::InvalidParameter => translate!("errors.invalid_parameter"),
        };

        write!(formatter, "{string}")
    }
}
