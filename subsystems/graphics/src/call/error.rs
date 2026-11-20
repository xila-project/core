//! Status codes returned to callers. They are part of the call ABI.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Error {
    InvalidArgumentsCount = 1,
    InvalidMemory = 2,
    Unavailable = 3,
    Unsupported = 4,
    InvalidHandle = 5,
    ResourceLimit = 6,
    InvalidValue = 7,
    NativeFailure = 8,
}

pub type Result<T> = core::result::Result<T, Error>;
