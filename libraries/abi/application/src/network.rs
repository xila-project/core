use crate::{Status, path_input};
use ::network::{
    IpAddress, IpEndpoint, IpListenEndpoint, Ipv6, Manager, Port, TcpSocket, UdpSocket,
};
use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_void},
    slice,
};
use task::block_on;

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
impl NetworkAddress {
    fn endpoint(self) -> Result<IpEndpoint, Status> {
        if self.scope_id != 0 || self.reserved != 0 {
            return Err(Status::InvalidArgument);
        }
        let ip = match self.family {
            XILA_ADDRESS_IPV4 if self.bytes[4..] == [0; 12] => {
                IpAddress::new_ipv4(self.bytes[..4].try_into().unwrap())
            }
            XILA_ADDRESS_IPV6 => IpAddress::IPv6(Ipv6::from_inner(self.bytes)),
            _ => return Err(Status::InvalidArgument),
        };
        Ok(IpEndpoint::new(ip, Port::new(self.port)))
    }
    fn new(ip: IpAddress, port: Port) -> Self {
        let mut bytes = [0; 16];
        let family = match ip {
            IpAddress::IPv4(ip) => {
                bytes[..4].copy_from_slice(&ip.into_inner());
                XILA_ADDRESS_IPV4
            }
            IpAddress::IPv6(ip) => {
                bytes = ip.into_inner();
                XILA_ADDRESS_IPV6
            }
        };
        Self {
            family,
            bytes,
            port: port.into_inner(),
            reserved: 0,
            scope_id: 0,
        }
    }
}

/// Host-granted network authority. Future host/port restrictions belong here.
pub struct Network {
    manager: &'static Manager,
    rights: u8,
}
impl Network {
    pub fn new(manager: &'static Manager, rights: u32) -> Result<Self, Status> {
        if rights & !7 != 0 {
            return Err(Status::InvalidArgument);
        }
        Ok(Self {
            manager,
            rights: rights as u8,
        })
    }
    fn require(&self, right: u32) -> Result<(), Status> {
        if u32::from(self.rights) & right == 0 {
            Err(Status::PermissionDenied)
        } else {
            Ok(())
        }
    }
}
/// Owned TCP connection. Its creation grants bidirectional I/O authority.
pub struct Tcp {
    inner: TcpSocket,
}
/// One pending connection, with no additional backlog. Accept replenishes the socket.
pub struct TcpListener {
    network: Network,
    address: NetworkAddress,
    inner: TcpSocket,
}
/// Owned UDP socket. Sending additionally requires CONNECT on its parent capability.
pub struct Udp {
    inner: UdpSocket,
    rights: u8,
}

macro_rules! ownership {
    ($name:ty) => {
        impl $name {
            pub fn into_raw(self) -> *mut Self {
                Box::into_raw(Box::new(self))
            }
            /// Reclaim an owned object exactly once.
            /// # Safety
            /// Pointer must be live and returned by `into_raw`.
            pub unsafe fn from_raw(raw: *mut Self) -> Box<Self> {
                unsafe { Box::from_raw(raw) }
            }
        }
    };
}
ownership!(Network);
ownership!(Tcp);
ownership!(TcpListener);
ownership!(Udp);
fn status(result: Result<(), Status>) -> Status {
    result.map_or_else(|e| e, |()| Status::Ok)
}
unsafe fn input<'a, T>(p: *const T) -> Result<&'a T, Status> {
    unsafe { p.as_ref() }.ok_or(Status::InvalidArgument)
}
unsafe fn object<'a, T>(p: *mut T) -> Result<&'a mut T, Status> {
    unsafe { p.as_mut() }.ok_or(Status::InvalidArgument)
}
unsafe fn output<T>(p: *mut T) -> Result<(), Status> {
    if p.is_null() {
        Err(Status::InvalidArgument)
    } else {
        Ok(())
    }
}
unsafe fn read_buffer<'a>(p: *const c_void, size: usize) -> Result<&'a [u8], Status> {
    if size > isize::MAX as usize || (size != 0 && p.is_null()) {
        return Err(Status::InvalidArgument);
    }
    Ok(if size == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(p.cast(), size) }
    })
}
unsafe fn write_buffer<'a>(p: *mut c_void, size: usize) -> Result<&'a mut [u8], Status> {
    if size > isize::MAX as usize || (size != 0 && p.is_null()) {
        return Err(Status::InvalidArgument);
    }
    Ok(if size == 0 {
        &mut []
    } else {
        unsafe { slice::from_raw_parts_mut(p.cast(), size) }
    })
}
// Small fixed socket buffers; no ABI-side staging buffer on I/O paths.
const BUFFER_SIZE: usize = 1024;
fn tcp_socket(network: &Network) -> Result<TcpSocket, Status> {
    block_on(
        network
            .manager
            .new_tcp_socket(BUFFER_SIZE, BUFFER_SIZE, None),
    )
    .map_err(Status::from)
}
fn listen(socket: &TcpSocket, address: NetworkAddress) -> Result<(), Status> {
    let endpoint = address.endpoint()?;
    block_on(socket.with_mutable(|s| {
        s.listen(IpListenEndpoint::new(Some(endpoint.address), endpoint.port).into_smoltcp())
    }))
    .map_err(|_| Status::InvalidArgument)
}

/// Release network authority; existing sockets remain valid. Null succeeds.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_network_close(network: *mut Network) -> Status {
    status(unsafe {
        if !network.is_null() {
            drop(Network::from_raw(network));
        }
        Ok(())
    })
}

/// Connect synchronously; timeout zero waits indefinitely, otherwise milliseconds.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_connect(
    network: *const Network,
    address: *const NetworkAddress,
    timeout_ms: u32,
    connection: *mut *mut Tcp,
) -> Status {
    status((|| unsafe {
        output(connection)?;
        let network = input(network)?;
        let endpoint = input(address)?.endpoint()?;
        network.require(XILA_NETWORK_CONNECT)?;
        let mut inner = tcp_socket(network)?;
        let result = block_on(async {
            if timeout_ms == 0 {
                return inner.connect(endpoint.address, endpoint.port).await;
            }
            use embassy_futures::select::{Either, select};
            let start = time::MonotonicClock::now(&time::EmbassyClock);
            match select(
                inner.connect(endpoint.address, endpoint.port),
                core::future::poll_fn(|_| {
                    if time::MonotonicClock::now(&time::EmbassyClock).saturating_sub(start)
                        >= core::time::Duration::from_millis(u64::from(timeout_ms))
                    {
                        core::task::Poll::Ready(())
                    } else {
                        core::task::Poll::Pending
                    }
                }),
            )
            .await
            {
                Either::First(result) => result,
                Either::Second(_) => Err(::network::Error::TimedOut),
            }
        });
        if let Err(error) = result {
            block_on(inner.close_forced());
            return Err(error.into());
        }
        connection.write(Tcp { inner }.into_raw());
        Ok(())
    })())
}

/// Bind and start a single-slot TCP listener. Port zero is rejected by Core.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_listen(
    network: *const Network,
    address: *const NetworkAddress,
    listener: *mut *mut TcpListener,
) -> Status {
    status((|| unsafe {
        output(listener)?;
        let network = input(network)?;
        let address = *input(address)?;
        address.endpoint()?;
        network.require(XILA_NETWORK_LISTEN)?;
        let mut inner = tcp_socket(network)?;
        if let Err(error) = listen(&inner, address) {
            block_on(inner.close_forced());
            return Err(error);
        }
        listener.write(
            TcpListener {
                network: Network::new(network.manager, u32::from(network.rights))?,
                address,
                inner,
            }
            .into_raw(),
        );
        Ok(())
    })())
}

/// Wait for a connection and replenish the listener. Run the interface runner independently.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_accept(
    listener: *mut TcpListener,
    connection: *mut *mut Tcp,
) -> Status {
    status((|| unsafe {
        output(connection)?;
        let listener = object(listener)?;
        listener.network.require(XILA_NETWORK_LISTEN)?;
        let endpoint = listener.address.endpoint()?;
        block_on(listener.inner.accept(Some(endpoint.address), endpoint.port))
            .map_err(Status::from)?;
        if block_on(listener.inner.get_remote_endpoint())
            .map_err(Status::from)?
            .is_none()
        {
            return Err(Status::ConnectionReset);
        }
        let mut replacement = tcp_socket(&listener.network)?;
        if let Err(error) = listen(&replacement, listener.address) {
            block_on(replacement.close_forced());
            return Err(error);
        }
        let inner = core::mem::replace(&mut listener.inner, replacement);
        connection.write(Tcp { inner }.into_raw());
        Ok(())
    })())
}

/// Abort and release the socket and its buffers. Null succeeds; queued data may be lost.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_close(socket: *mut Tcp) -> Status {
    status(unsafe {
        if !socket.is_null() {
            let mut socket = Tcp::from_raw(socket);
            block_on(socket.inner.close_forced());
        }
        Ok(())
    })
}

/// Abort and release the socket and its buffers. Null succeeds; queued data may be lost.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_listener_close(socket: *mut TcpListener) -> Status {
    status(unsafe {
        if !socket.is_null() {
            let mut socket = TcpListener::from_raw(socket);
            block_on(socket.inner.close_forced());
        }
        Ok(())
    })
}

/// Gracefully shut down writing. Only XILA_SHUTDOWN_WRITE is supported; reads remain valid.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_shutdown(socket: *mut Tcp, direction: u32) -> Status {
    status((|| unsafe {
        let socket = object(socket)?;
        if direction != XILA_SHUTDOWN_WRITE {
            return Err(Status::InvalidArgument);
        }
        block_on(socket.inner.close());
        Ok(())
    })())
}

/// Blocking direct-buffer I/O. Zero size permits null; send may be partial; receive zero means EOF.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_send(
    socket: *mut Tcp,
    buffer: *const c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    status((|| unsafe {
        output(count)?;
        let socket = object(socket)?;
        let buffer = read_buffer(buffer, size)?;
        let n = if size == 0 {
            0
        } else {
            block_on(socket.inner.write(buffer)).map_err(Status::from)?
        };
        count.write(n);
        Ok(())
    })())
}

/// Blocking direct-buffer I/O. Zero size permits null; send may be partial; receive zero means EOF.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_receive(
    socket: *mut Tcp,
    buffer: *mut c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    status((|| unsafe {
        output(count)?;
        let socket = object(socket)?;
        let buffer = write_buffer(buffer, size)?;
        let n = if size == 0 {
            0
        } else {
            block_on(socket.inner.read(buffer)).map_err(Status::from)?
        };
        count.write(n);
        Ok(())
    })())
}

/// Read the connection endpoint.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_local_address(
    socket: *const Tcp,
    address: *mut NetworkAddress,
) -> Status {
    status((|| unsafe {
        output(address)?;
        let socket = input(socket)?;
        let (ip, port) = block_on(socket.inner.get_local_endpoint())
            .map_err(Status::from)?
            .ok_or(Status::NotConnected)?;
        address.write(NetworkAddress::new(ip, port));
        Ok(())
    })())
}

/// Read the connection endpoint.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_tcp_peer_address(
    socket: *const Tcp,
    address: *mut NetworkAddress,
) -> Status {
    status((|| unsafe {
        output(address)?;
        let socket = input(socket)?;
        let (ip, port) = block_on(socket.inner.get_remote_endpoint())
            .map_err(Status::from)?
            .ok_or(Status::NotConnected)?;
        address.write(NetworkAddress::new(ip, port));
        Ok(())
    })())
}

/// Bind a UDP socket. Port zero is rejected by Core; one datagram per direction is buffered.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_udp_bind(
    network: *const Network,
    address: *const NetworkAddress,
    socket: *mut *mut Udp,
) -> Status {
    status((|| unsafe {
        output(socket)?;
        let network = input(network)?;
        let endpoint = input(address)?.endpoint()?;
        network.require(XILA_NETWORK_LISTEN)?;
        let inner = block_on(
            network
                .manager
                .new_udp_socket(BUFFER_SIZE, BUFFER_SIZE, 1, 1, None),
        )
        .map_err(Status::from)?;
        let result = block_on(inner.with_mutable(|s| {
            s.bind(IpListenEndpoint::new(Some(endpoint.address), endpoint.port).into_smoltcp())
        }));
        if result.is_err() {
            block_on(inner.close());
            return Err(Status::InvalidArgument);
        }
        socket.write(
            Udp {
                inner,
                rights: network.rights,
            }
            .into_raw(),
        );
        Ok(())
    })())
}

/// Release a UDP socket. Null succeeds.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_udp_close(socket: *mut Udp) -> Status {
    status(unsafe {
        if !socket.is_null() {
            let Udp { inner, .. } = *Udp::from_raw(socket);
            block_on(inner.close());
        }
        Ok(())
    })
}

/// Nonblocking atomic datagram send, without logging or staging allocation.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_udp_send_to(
    socket: *mut Udp,
    buffer: *const c_void,
    size: usize,
    address: *const NetworkAddress,
    count: *mut usize,
) -> Status {
    status((|| unsafe {
        output(count)?;
        let socket = object(socket)?;
        let endpoint = input(address)?.endpoint()?;
        let buffer = read_buffer(buffer, size)?;
        if u32::from(socket.rights) & XILA_NETWORK_CONNECT == 0 {
            return Err(Status::PermissionDenied);
        }
        block_on(socket.inner.with_mutable(|s| {
            if size > s.payload_send_capacity() {
                return Err(Status::Overflow);
            }
            s.send_slice(buffer, endpoint.into_smoltcp())
                .map_err(|e| match e {
                    smoltcp::socket::udp::SendError::BufferFull => Status::WouldBlock,
                    smoltcp::socket::udp::SendError::Unaddressable => Status::HostUnreachable,
                })
        }))?;
        count.write(size);
        Ok(())
    })())
}

/// Nonblocking datagram receive. Overflow preserves the datagram; outputs change only on success.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_udp_receive_from(
    socket: *mut Udp,
    buffer: *mut c_void,
    size: usize,
    address: *mut NetworkAddress,
    count: *mut usize,
) -> Status {
    status((|| {
        unsafe {
            output(count)?;
            output(address)?;
            let socket = object(socket)?;
            let buffer = write_buffer(buffer, size)?;
            let (n, endpoint) = block_on(socket.inner.with_mutable(|s| {
                // Peek first so overflow preserves the datagram and caller buffer.
                let (data, _) = s.peek().map_err(|_| Status::WouldBlock)?;
                if data.len() > size {
                    return Err(Status::Overflow);
                }
                s.recv_slice(buffer).map_err(|_| Status::InputOutput)
            }))?;
            address.write(NetworkAddress::new(
                IpAddress::from_smoltcp(&endpoint.endpoint.addr),
                Port::new(endpoint.endpoint.port),
            ));
            count.write(n);
            Ok(())
        }
    })())
}

/// Resolve one address family synchronously. Count is capacity in/results out; capacity zero queries size. Overflow leaves outputs unchanged. DNS allocates inside Core.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xila_network_resolve(
    network: *const Network,
    host: *const c_char,
    host_length: usize,
    family: u32,
    addresses: *mut NetworkAddress,
    count: *mut usize,
) -> Status {
    status((|| unsafe {
        let network = input(network)?;
        let capacity = *input(count)?;
        if capacity > isize::MAX as usize / core::mem::size_of::<NetworkAddress>()
            || (capacity != 0 && addresses.is_null())
        {
            return Err(Status::InvalidArgument);
        }
        let host = path_input(host, host_length)?;
        if host.is_empty() || host.contains('\0') {
            return Err(Status::InvalidArgument);
        }
        let kind = match family {
            XILA_ADDRESS_IPV4 => ::network::DnsQueryKind::A,
            XILA_ADDRESS_IPV6 => ::network::DnsQueryKind::Aaaa,
            _ => return Err(Status::InvalidArgument),
        };
        network.require(XILA_NETWORK_RESOLVE)?;
        let resolved =
            block_on(network.manager.resolve(host, kind, false, None)).map_err(Status::from)?;
        let matches = |ip: &&IpAddress| {
            matches!(
                (family, ip),
                (XILA_ADDRESS_IPV4, IpAddress::IPv4(_)) | (XILA_ADDRESS_IPV6, IpAddress::IPv6(_))
            )
        };
        let needed = resolved.iter().filter(matches).count();
        if capacity == 0 {
            count.write(needed);
            return Ok(());
        }
        if needed > capacity {
            return Err(Status::Overflow);
        }
        for (index, ip) in resolved.iter().filter(matches).enumerate() {
            addresses
                .add(index)
                .write(NetworkAddress::new(ip.clone(), Port::new(0)));
        }
        count.write(needed);
        Ok(())
    })())
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    #[test]
    fn addresses_and_layout() {
        for (ip, port) in [
            (IpAddress::new_ipv4([192, 0, 2, 1]), Port::new(1234)),
            (
                IpAddress::new_ipv6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]),
                Port::new(65535),
            ),
        ] {
            let address = NetworkAddress::new(ip.clone(), port);
            assert_eq!(address.endpoint(), Ok(IpEndpoint::new(ip, port)));
            for invalid in [
                NetworkAddress {
                    family: 99,
                    ..address
                },
                NetworkAddress {
                    reserved: 1,
                    ..address
                },
                NetworkAddress {
                    scope_id: 1,
                    ..address
                },
            ] {
                assert_eq!(invalid.endpoint(), Err(Status::InvalidArgument));
            }
        }
        let mut invalid = NetworkAddress::new(IpAddress::new_ipv4([127, 0, 0, 1]), Port::new(80));
        invalid.bytes[15] = 1;
        assert_eq!(invalid.endpoint(), Err(Status::InvalidArgument));
        assert_eq!(core::mem::size_of::<NetworkAddress>(), 28);
        std::println!(
            "Network={} Tcp={} TcpListener={} Udp={} Address={}",
            core::mem::size_of::<Network>(),
            core::mem::size_of::<Tcp>(),
            core::mem::size_of::<TcpListener>(),
            core::mem::size_of::<Udp>(),
            core::mem::size_of::<NetworkAddress>()
        );
    }
    #[test]
    fn null_outputs() {
        unsafe {
            assert_eq!(
                xila_tcp_connect(
                    core::ptr::null(),
                    core::ptr::null(),
                    0,
                    core::ptr::null_mut()
                ),
                Status::InvalidArgument
            );
            assert_eq!(
                xila_udp_receive_from(
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    1,
                    core::ptr::null_mut(),
                    core::ptr::null_mut()
                ),
                Status::InvalidArgument
            );
        }
        assert_eq!(Status::from(::network::Error::TimedOut), Status::TimedOut);
        assert_eq!(
            Status::from(::network::Error::ResourceBusy),
            Status::WouldBlock
        );
    }
}
