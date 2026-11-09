use core::{fmt::Display, num::NonZeroU8};
use embedded_i18n::t;
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
            Error::NotFound => write!(f, "{}", t!("errors.not_found")),
            Error::PermissionDenied => write!(f, "{}", t!("errors.permission_denied")),
            Error::ConnectionRefused => write!(f, "{}", t!("errors.connection_refused")),
            Error::ConnectionReset => write!(f, "{}", t!("errors.connection_reset")),
            Error::HostUnreachable => write!(f, "{}", t!("errors.host_unreachable")),
            Error::NetworkUnreachable => write!(f, "{}", t!("errors.network_unreachable")),
            Error::ConnectionAborted => write!(f, "{}", t!("errors.connection_aborted")),
            Error::NotConnected => write!(f, "{}", t!("errors.not_connected")),
            Error::AddressInUse => write!(f, "{}", t!("errors.address_in_use")),
            Error::AddressNotAvailable => write!(f, "{}", t!("errors.address_not_available")),
            Error::NetworkDown => write!(f, "{}", t!("errors.network_down")),
            Error::BrokenPipe => write!(f, "{}", t!("errors.broken_pipe")),
            Error::AlreadyExists => write!(f, "{}", t!("errors.already_exists")),
            Error::WouldBlock => write!(f, "{}", t!("errors.would_block")),
            Error::InvalidInput => write!(f, "{}", t!("errors.invalid_input")),
            Error::InvalidData => write!(f, "{}", t!("errors.invalid_data")),
            Error::TimedOut => write!(f, "{}", t!("errors.timed_out")),
            Error::WriteZero => write!(f, "{}", t!("errors.write_zero")),
            Error::StorageFull => write!(f, "{}", t!("errors.storage_full")),
            Error::ResourceBusy => write!(f, "{}", t!("errors.resource_busy")),
            Error::Deadlock => write!(f, "{}", t!("errors.deadlock")),
            Error::Interrupted => write!(f, "{}", t!("errors.interrupted")),
            Error::Unsupported => write!(f, "{}", t!("errors.unsupported_operation")),
            Error::UnexpectedEndOfFile => write!(f, "{}", t!("errors.unexpected_end_of_file")),
            Error::OutOfMemory => write!(f, "{}", t!("errors.out_of_memory")),
            Error::Pending => write!(
                f,
                "{}",
                t!("errors.in_progress_operation_not_completed_yet")
            ),
            Error::UnsupportedProtocol => {
                write!(f, "{}", t!("errors.unsupported_protocol_used_in_operation"))
            }
            Error::InvalidIdentifier => {
                write!(
                    f,
                    "{}",
                    t!("errors.invalid_identifier_provided_for_operation")
                )
            }
            Error::DuplicateIdentifier => {
                write!(
                    f,
                    "{}",
                    t!("errors.duplicate_identifier_found_in_operation")
                )
            }
            Error::FailedToGenerateSeed(e) => {
                write!(f, "{}", t!("errors.failed_to_generate_seed", arg0 = e))
            }
            Error::FailedToSpawnNetworkTask(e) => {
                write!(f, "{}", t!("errors.failed_to_spawn_network_task", arg0 = e))
            }
            Error::InvalidName => write!(f, "{}", t!("errors.invalid_name")),
            Error::NameTooLong => write!(f, "{}", t!("errors.name_too_long")),
            Error::Failed => write!(f, "{}", t!("errors.failed")),
            Error::InvalidState => write!(f, "{}", t!("errors.invalid_state_for_operation")),
            Error::InvalidPort => write!(f, "{}", t!("errors.invalid_port_specified")),
            Error::NoRoute => write!(f, "{}", t!("errors.no_route_to_host")),
            Error::Truncated => write!(f, "{}", t!("errors.truncated_packet_received")),
            Error::SocketNotBound => write!(f, "{}", t!("errors.socket_not_bound")),
            Error::PacketTooLarge => write!(f, "{}", t!("errors.packet_too_large_to_send")),
            Error::InvalidEndpoint => write!(f, "{}", t!("errors.invalid_endpoint_specified")),
            Error::FailedToMountDevice(e) => {
                write!(f, "{}", t!("errors.failed_to_mount_device", arg0 = e))
            }
            Error::NoFreeSlot => write!(f, "{}", t!("errors.no_free_slot_available")),
            Error::Other => write!(f, "{}", t!("errors.other_error_occurred")),
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
