use core::fmt::Display;
use embedded_i18n::t;

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

impl From<lvgl_rust::Error> for Error {
    fn from(error: lvgl_rust::Error) -> Self {
        match error {
            lvgl_rust::Error::Dead => Error::InvalidReference,
            lvgl_rust::Error::RegistryFull | lvgl_rust::Error::OutOfMemory => Error::OutOfMemory,
            lvgl_rust::Error::Reentrant | lvgl_rust::Error::Busy => Error::AlreadyInUse,
            lvgl_rust::Error::NotInitialized => Error::NotInitialized,
            lvgl_rust::Error::AlreadyOwned | lvgl_rust::Error::OtherOwner => Error::AlreadyInUse,
            _ => Error::InvalidParameter,
        }
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        let string = match self {
            Error::InvalidReference => t!("errors.invalid_reference"),
            Error::AlreadyInitialized => t!("errors.already_initialized"),
            Error::NotInitialized => t!("errors.not_initialized"),
            Error::OutOfMemory => t!("errors.out_of_memory"),
            Error::AlreadyInUse => t!("errors.already_in_use"),
            Error::FailedToRegister => t!("errors.failed_to_register"),
            Error::FailedToGetResolution => t!("errors.failed_to_get_resolution"),
            Error::NotRegistered => t!("errors.not_registered"),
            Error::NotAvailable => t!("errors.not_available"),
            Error::FailedToCreateObject => t!("errors.failed_to_create_object"),
            Error::InvalidWindowIdentifier => t!("errors.invalid_window_identifier"),
            Error::InvalidParameter => t!("errors.invalid_parameter"),
        };

        write!(formatter, "{string}")
    }
}
