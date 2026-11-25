#[cfg(feature = "network")]
use ::network::{Manager, TcpSocket, UdpSocket};

pub const XILA_NETWORK_CONNECT: u32 = 1;
pub const XILA_NETWORK_LISTEN: u32 = 2;
pub const XILA_NETWORK_RESOLVE: u32 = 4;
pub const XILA_ADDRESS_IPV4: u32 = 4;
pub const XILA_ADDRESS_IPV6: u32 = 6;
pub const XILA_SHUTDOWN_WRITE: u32 = 1;

/// Fixed-width address. Bytes are in network order; integer fields are host order.
/// IPv4 uses the first four bytes and requires the remaining bytes to be zero.
/// Scope must be zero: Core does not support scoped IPv6 endpoints.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkAddress {
    pub family: u32,
    pub bytes: [u8; 16],
    pub port: u16,
    pub reserved: u16,
    pub scope_id: u32,
}
/// Host-granted network authority. Future host/port restrictions belong here.
pub struct Network {
    #[cfg(not(feature = "network"))]
    _unavailable: core::convert::Infallible,
    #[cfg(feature = "network")]
    pub(crate) manager: &'static Manager,
    #[cfg(feature = "network")]
    pub(crate) rights: u8,
}
/// Owned TCP connection. Its creation grants bidirectional I/O authority.
pub struct Tcp {
    #[cfg(not(feature = "network"))]
    _unavailable: core::convert::Infallible,
    #[cfg(feature = "network")]
    pub(crate) inner: TcpSocket,
}
/// One pending connection, with no additional backlog. Accept replenishes the socket.
pub struct TcpListener {
    #[cfg(not(feature = "network"))]
    _unavailable: core::convert::Infallible,
    #[cfg(feature = "network")]
    pub(crate) network: Network,
    #[cfg(feature = "network")]
    pub(crate) address: NetworkAddress,
    #[cfg(feature = "network")]
    pub(crate) inner: TcpSocket,
    /// Embassy-time tick by which a reset left pending by a poll must have been sent;
    /// the listener listens again after it even if the reset is still pending. Zero
    /// when no reset is pending. Atomic because concurrent poll waits are allowed.
    #[cfg(feature = "network")]
    pub(crate) reset_deadline: portable_atomic::AtomicU64,
}
/// Owned UDP socket. Sending additionally requires CONNECT on its parent capability.
pub struct Udp {
    #[cfg(not(feature = "network"))]
    _unavailable: core::convert::Infallible,
    #[cfg(feature = "network")]
    pub(crate) inner: UdpSocket,
    #[cfg(feature = "network")]
    pub(crate) rights: u8,
    /// `XILA_ADDRESS_IPV4` or `XILA_ADDRESS_IPV6`: the family it was bound with, the
    /// only one it sends to and receives from.
    #[cfg(feature = "network")]
    pub(crate) family: u32,
}
