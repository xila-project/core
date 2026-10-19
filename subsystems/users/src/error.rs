use core::fmt::Display;
use embedded_i18n::translate;

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
                write!(formatter, translate!("Duplicate group identifier"))
            }
            Self::DuplicateGroupName => {
                write!(formatter, translate!("Duplicate group name"))
            }
            Self::DuplicateUserIdentifier => {
                write!(formatter, translate!("Duplicate user identifier"))
            }
            Self::DuplicateUserName => {
                write!(formatter, translate!("Duplicate user name"))
            }
            Self::InvalidGroupIdentifier => {
                write!(formatter, translate!("Invalid group identifier"))
            }
            Self::InvalidUserIdentifier => {
                write!(formatter, translate!("Invalid user identifier"))
            }
            Self::TooManyGroups => {
                write!(formatter, translate!("Too many groups"))
            }
            Self::TooManyUsers => {
                write!(formatter, translate!("Too many users"))
            }
            Self::PoisonedLock => {
                write!(formatter, translate!("Poisoned lock"))
            }
            Self::NotInitialized => {
                write!(formatter, translate!("Not initialized"))
            }
            Self::AlreadyInitialized => {
                write!(formatter, translate!("Already initialized"))
            }
        }
    }
}
