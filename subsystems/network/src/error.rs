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
            Error::NotFound => write!(f, translate!("errors.not_found")),
            Error::PermissionDenied => write!(f, translate!("errors.permission_denied")),
            Error::ConnectionRefused => write!(f, translate!("errors.connection_refused")),
            Error::ConnectionReset => write!(f, translate!("errors.connection_reset")),
            Error::HostUnreachable => write!(f, translate!("errors.host_unreachable")),
            Error::NetworkUnreachable => write!(f, translate!("errors.network_unreachable")),
            Error::ConnectionAborted => write!(f, translate!("errors.connection_aborted")),
            Error::NotConnected => write!(f, translate!("errors.not_connected")),
            Error::AddressInUse => write!(f, translate!("errors.address_in_use")),
            Error::AddressNotAvailable => write!(f, translate!("errors.address_not_available")),
            Error::NetworkDown => write!(f, translate!("errors.network_down")),
            Error::BrokenPipe => write!(f, translate!("errors.broken_pipe")),
            Error::AlreadyExists => write!(f, translate!("errors.already_exists")),
            Error::WouldBlock => write!(f, translate!("errors.would_block")),
            Error::InvalidInput => write!(f, translate!("errors.invalid_input")),
            Error::InvalidData => write!(f, translate!("errors.invalid_data")),
            Error::TimedOut => write!(f, translate!("errors.timed_out")),
            Error::WriteZero => write!(f, translate!("errors.write_zero")),
            Error::StorageFull => write!(f, translate!("errors.storage_full")),
            Error::ResourceBusy => write!(f, translate!("errors.resource_busy")),
            Error::Deadlock => write!(f, translate!("errors.deadlock")),
            Error::Interrupted => write!(f, translate!("errors.interrupted")),
            Error::Unsupported => write!(f, translate!("errors.unsupported_operation")),
            Error::UnexpectedEndOfFile => write!(f, translate!("errors.unexpected_end_of_file")),
            Error::OutOfMemory => write!(f, translate!("errors.out_of_memory")),
            Error::Pending => write!(
                f,
                translate!("errors.in_progress_operation_not_completed_yet")
            ),
            Error::UnsupportedProtocol => {
                write!(
                    f,
                    translate!("errors.unsupported_protocol_used_in_operation")
                )
            }
            Error::InvalidIdentifier => {
                write!(
                    f,
                    translate!("errors.invalid_identifier_provided_for_operation")
                )
            }
            Error::DuplicateIdentifier => {
                write!(
                    f,
                    translate!("errors.duplicate_identifier_found_in_operation")
                )
            }
            Error::FailedToGenerateSeed(e) => {
                write!(f, translate!("errors.failed_to_generate_seed"), e)
            }
            Error::FailedToSpawnNetworkTask(e) => {
                write!(f, translate!("errors.failed_to_spawn_network_task"), e)
            }
            Error::InvalidName => write!(f, translate!("errors.invalid_name")),
            Error::NameTooLong => write!(f, translate!("errors.name_too_long")),
            Error::Failed => write!(f, translate!("errors.failed")),
            Error::InvalidState => write!(f, translate!("errors.invalid_state_for_operation")),
            Error::InvalidPort => write!(f, translate!("errors.invalid_port_specified")),
            Error::NoRoute => write!(f, translate!("errors.no_route_to_host")),
            Error::Truncated => write!(f, translate!("errors.truncated_packet_received")),
            Error::SocketNotBound => write!(f, translate!("errors.socket_not_bound")),
            Error::PacketTooLarge => write!(f, translate!("errors.packet_too_large_to_send")),
            Error::InvalidEndpoint => write!(f, translate!("errors.invalid_endpoint_specified")),
            Error::FailedToMountDevice(e) => {
                write!(f, translate!("errors.failed_to_mount_device"), e)
            }
            Error::NoFreeSlot => write!(f, translate!("errors.no_free_slot_available")),
            Error::Other => write!(f, translate!("errors.other_error_occurred")),
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
