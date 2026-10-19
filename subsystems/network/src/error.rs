use core::{fmt::Display, num::NonZeroU8};
use embedded_i18n::translate;
use smoltcp::socket::{dns, icmp, udp};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Error {
    NotFound = 1,
    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    HostUnreachable,
    NetworkUnreachable,
    ConnectionAborted,
    NotConnected,
    AddressInUse,
    AddressNotAvailable,
    NetworkDown,
    BrokenPipe,
    AlreadyExists,
    WouldBlock,
    InvalidInput,
    InvalidData,
    TimedOut,
    WriteZero,
    StorageFull,
    ResourceBusy,
    Deadlock,
    Interrupted,
    Unsupported,
    UnexpectedEndOfFile,
    OutOfMemory,
    Pending,
    UnsupportedProtocol,
    InvalidIdentifier,
    DuplicateIdentifier,
    FailedToGenerateSeed(file_system::Error),
    FailedToSpawnNetworkTask(task::Error),
    // - DNS
    InvalidName,
    NameTooLong,
    Failed,
    // - Accept / Connect
    InvalidState,
    InvalidPort,
    NoRoute,
    // Udp
    Truncated,
    SocketNotBound,
    PacketTooLarge,
    InvalidEndpoint,

    FailedToMountDevice(virtual_file_system::Error),

    NoFreeSlot,

    Other,
}

impl core::error::Error for Error {}

impl Error {
    pub const fn get_discriminant(&self) -> NonZeroU8 {
        unsafe { *(self as *const Self as *const NonZeroU8) }
    }
}

impl From<Error> for NonZeroU8 {
    fn from(value: Error) -> Self {
        value.get_discriminant()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotFound => write!(f, translate!("Not found")),
            Error::PermissionDenied => write!(f, translate!("Permission denied")),
            Error::ConnectionRefused => write!(f, translate!("Connection refused")),
            Error::ConnectionReset => write!(f, translate!("Connection reset")),
            Error::HostUnreachable => write!(f, translate!("Host unreachable")),
            Error::NetworkUnreachable => write!(f, translate!("Network unreachable")),
            Error::ConnectionAborted => write!(f, translate!("Connection aborted")),
            Error::NotConnected => write!(f, translate!("Not connected")),
            Error::AddressInUse => write!(f, translate!("Address in use")),
            Error::AddressNotAvailable => write!(f, translate!("Address not available")),
            Error::NetworkDown => write!(f, translate!("Network down")),
            Error::BrokenPipe => write!(f, translate!("Broken pipe")),
            Error::AlreadyExists => write!(f, translate!("Already exists")),
            Error::WouldBlock => write!(f, translate!("Would block")),
            Error::InvalidInput => write!(f, translate!("Invalid input")),
            Error::InvalidData => write!(f, translate!("Invalid data")),
            Error::TimedOut => write!(f, translate!("Timed out")),
            Error::WriteZero => write!(f, translate!("Write zero")),
            Error::StorageFull => write!(f, translate!("Storage full")),
            Error::ResourceBusy => write!(f, translate!("Resource busy")),
            Error::Deadlock => write!(f, translate!("Deadlock")),
            Error::Interrupted => write!(f, translate!("Interrupted")),
            Error::Unsupported => write!(f, translate!("Unsupported operation")),
            Error::UnexpectedEndOfFile => write!(f, translate!("Unexpected end of file")),
            Error::OutOfMemory => write!(f, translate!("Out of memory")),
            Error::Pending => write!(f, translate!("In progress operation not completed yet")),
            Error::UnsupportedProtocol => {
                write!(f, translate!("Unsupported protocol used in operation"))
            }
            Error::InvalidIdentifier => {
                write!(f, translate!("Invalid identifier provided for operation"))
            }
            Error::DuplicateIdentifier => {
                write!(f, translate!("Duplicate identifier found in operation"))
            }
            Error::FailedToGenerateSeed(e) => {
                write!(f, translate!("Failed to generate seed: {}"), e)
            }
            Error::FailedToSpawnNetworkTask(e) => {
                write!(f, translate!("Failed to spawn network task: {}"), e)
            }
            Error::InvalidName => write!(f, translate!("Invalid name")),
            Error::NameTooLong => write!(f, translate!("Name too long")),
            Error::Failed => write!(f, translate!("Failed")),
            Error::InvalidState => write!(f, translate!("Invalid state for operation")),
            Error::InvalidPort => write!(f, translate!("Invalid port specified")),
            Error::NoRoute => write!(f, translate!("No route to host")),
            Error::Truncated => write!(f, translate!("Truncated packet received")),
            Error::SocketNotBound => write!(f, translate!("Socket not bound")),
            Error::PacketTooLarge => write!(f, translate!("Packet too large to send")),
            Error::InvalidEndpoint => write!(f, translate!("Invalid endpoint specified")),
            Error::FailedToMountDevice(e) => {
                write!(f, translate!("Failed to mount device: {}"), e)
            }
            Error::NoFreeSlot => write!(f, translate!("No free slot available")),
            Error::Other => write!(f, translate!("Other error occurred")),
        }
    }
}

impl From<dns::StartQueryError> for Error {
    fn from(e: dns::StartQueryError) -> Self {
        match e {
            dns::StartQueryError::InvalidName => Error::InvalidName,
            dns::StartQueryError::NameTooLong => Error::NameTooLong,
            dns::StartQueryError::NoFreeSlot => Error::NoFreeSlot,
        }
    }
}

impl From<dns::GetQueryResultError> for Error {
    fn from(e: dns::GetQueryResultError) -> Self {
        match e {
            dns::GetQueryResultError::Pending => Error::Pending,
            dns::GetQueryResultError::Failed => Error::Failed,
        }
    }
}

impl From<icmp::SendError> for Error {
    fn from(e: icmp::SendError) -> Self {
        match e {
            icmp::SendError::Unaddressable => Error::InvalidEndpoint,
            icmp::SendError::BufferFull => Error::ResourceBusy,
        }
    }
}

impl From<icmp::BindError> for Error {
    fn from(e: icmp::BindError) -> Self {
        match e {
            icmp::BindError::InvalidState => Error::InvalidState,
            icmp::BindError::Unaddressable => Error::NoRoute,
        }
    }
}

impl From<udp::BindError> for Error {
    fn from(e: udp::BindError) -> Self {
        match e {
            udp::BindError::InvalidState => Error::InvalidState,
            udp::BindError::Unaddressable => Error::NoRoute,
        }
    }
}
