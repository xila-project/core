use core::fmt::Display;
use embedded_i18n::t;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub enum Error {
    DuplicateGroupIdentifier,
    DuplicateGroupName,
    DuplicateUserIdentifier,
    DuplicateUserName,
    InvalidGroupIdentifier,
    InvalidUserIdentifier,
    TooManyGroups,
    TooManyUsers,
    PoisonedLock,
    NotInitialized,
    AlreadyInitialized,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Self::DuplicateGroupIdentifier => {
                write!(formatter, "{}", t!("errors.duplicate_group_identifier"))
            }
            Self::DuplicateGroupName => {
                write!(formatter, "{}", t!("errors.duplicate_group_name"))
            }
            Self::DuplicateUserIdentifier => {
                write!(formatter, "{}", t!("errors.duplicate_user_identifier"))
            }
            Self::DuplicateUserName => {
                write!(formatter, "{}", t!("errors.duplicate_user_name"))
            }
            Self::InvalidGroupIdentifier => {
                write!(formatter, "{}", t!("errors.invalid_group_identifier"))
            }
            Self::InvalidUserIdentifier => {
                write!(formatter, "{}", t!("errors.invalid_user_identifier"))
            }
            Self::TooManyGroups => {
                write!(formatter, "{}", t!("errors.too_many_groups"))
            }
            Self::TooManyUsers => {
                write!(formatter, "{}", t!("errors.too_many_users"))
            }
            Self::PoisonedLock => {
                write!(formatter, "{}", t!("errors.poisoned_lock"))
            }
            Self::NotInitialized => {
                write!(formatter, "{}", t!("errors.not_initialized"))
            }
            Self::AlreadyInitialized => {
                write!(formatter, "{}", t!("errors.already_initialized"))
            }
        }
    }
}
