//! Network errors are the shared I/O vocabulary plus a typed resolver error.
//!
//! The translations from smoltcp live here, one per operation, because the same
//! smoltcp error means different things for a bind, a send and a lookup.

pub use file_system::{IntoIoErrorKind, IoErrorKind, Result, ResultExt, io_error};
pub use io_error::{GetAddrInfoError, ResolveError};

use smoltcp::socket::{dns, icmp, udp};

/// Result of a name resolution.
pub type ResolveResult<T> = core::result::Result<T, ResolveError>;

/// A DNS query could not be started.
pub(crate) fn dns_start_error(error: dns::StartQueryError) -> ResolveError {
    match error {
        dns::StartQueryError::InvalidName => ResolveError::InvalidName,
        dns::StartQueryError::NameTooLong => ResolveError::NameTooLong,
        dns::StartQueryError::NoFreeSlot => ResolveError::NoQuerySlots,
    }
}

/// A finished DNS query failed. smoltcp reports no reason, so this stays the
/// generic failure: it is never reported as a confirmed negative answer.
pub(crate) fn dns_result_error(error: dns::GetQueryResultError) -> ResolveError {
    match error {
        dns::GetQueryResultError::Pending | dns::GetQueryResultError::Failed => {
            ResolveError::Failure
        }
    }
}

/// An ICMP or UDP bind failed: the socket is already open, or the local
/// address cannot be used.
pub(crate) fn icmp_bind_error(error: icmp::BindError) -> IoErrorKind {
    match error {
        icmp::BindError::InvalidState => IoErrorKind::InvalidState,
        icmp::BindError::Unaddressable => IoErrorKind::AddressNotAvailable,
    }
}

/// See [`icmp_bind_error`].
pub(crate) fn udp_bind_error(error: udp::BindError) -> IoErrorKind {
    match error {
        udp::BindError::InvalidState => IoErrorKind::InvalidState,
        udp::BindError::Unaddressable => IoErrorKind::AddressNotAvailable,
    }
}

/// An ICMP send was refused by smoltcp.
///
/// `bound` says whether the socket has a local binding and `destination_given`
/// whether the caller supplied a usable destination: smoltcp reports both
/// problems as the same `Unaddressable`.
pub(crate) fn icmp_send_error(
    error: icmp::SendError,
    bound: bool,
    destination_given: bool,
) -> IoErrorKind {
    match error {
        icmp::SendError::BufferFull => IoErrorKind::WouldBlock,
        icmp::SendError::Unaddressable => unaddressable_send(bound, destination_given),
    }
}

/// A UDP send was refused by smoltcp. See [`icmp_send_error`].
pub(crate) fn udp_send_error(
    error: udp::SendError,
    bound: bool,
    destination_given: bool,
) -> IoErrorKind {
    match error {
        udp::SendError::BufferFull => IoErrorKind::WouldBlock,
        udp::SendError::Unaddressable => unaddressable_send(bound, destination_given),
    }
}

fn unaddressable_send(bound: bool, destination_given: bool) -> IoErrorKind {
    if !destination_given {
        IoErrorKind::DestinationRequired
    } else if !bound {
        IoErrorKind::InvalidState
    } else {
        IoErrorKind::HostUnreachable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_failures_stay_distinct() {
        assert_eq!(
            dns_start_error(dns::StartQueryError::InvalidName),
            ResolveError::InvalidName
        );
        assert_eq!(
            dns_start_error(dns::StartQueryError::NameTooLong),
            ResolveError::NameTooLong
        );
        assert_eq!(
            dns_start_error(dns::StartQueryError::NoFreeSlot),
            ResolveError::NoQuerySlots
        );
        // The opaque smoltcp failure is never a confirmed negative answer.
        assert_eq!(
            dns_result_error(dns::GetQueryResultError::Failed),
            ResolveError::Failure
        );
    }

    #[test]
    fn bind_errors_split_state_and_address() {
        assert_eq!(
            icmp_bind_error(icmp::BindError::InvalidState),
            IoErrorKind::InvalidState
        );
        assert_eq!(
            icmp_bind_error(icmp::BindError::Unaddressable),
            IoErrorKind::AddressNotAvailable
        );
        assert_eq!(
            udp_bind_error(udp::BindError::InvalidState),
            IoErrorKind::InvalidState
        );
        assert_eq!(
            udp_bind_error(udp::BindError::Unaddressable),
            IoErrorKind::AddressNotAvailable
        );
    }

    #[test]
    fn unaddressable_send_depends_on_the_cause() {
        let error = udp::SendError::Unaddressable;
        assert_eq!(
            udp_send_error(error, true, false),
            IoErrorKind::DestinationRequired
        );
        assert_eq!(
            udp_send_error(error, false, true),
            IoErrorKind::InvalidState
        );
        assert_eq!(
            udp_send_error(error, true, true),
            IoErrorKind::HostUnreachable
        );
        assert_eq!(
            icmp_send_error(icmp::SendError::BufferFull, true, true),
            IoErrorKind::WouldBlock
        );
    }
}
