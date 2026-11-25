use crate::{
    Network, NetworkAddress, Status, Tcp, TcpListener, Udp, XILA_ADDRESS_IPV4, XILA_ADDRESS_IPV6,
    XILA_NETWORK_CONNECT, XILA_NETWORK_LISTEN, XILA_NETWORK_RESOLVE, XILA_SHUTDOWN_WRITE,
    path_input,
    registry::{self, Handle},
    suspend_on, suspend_on_cancellable,
};
use ::network::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv6, Manager, Port, TcpSocket};
use alloc::boxed::Box;
use core::{
    ffi::{c_char, c_void},
    slice,
};

impl NetworkAddress {
    fn endpoint(self) -> Result<IpEndpoint, Status> {
        if self.scope_id != 0 || self.reserved != 0 {
            return Err(Status::InvalidArgument);
        }
        let ip = match self.family {
            XILA_ADDRESS_IPV4 if self.bytes[4..] == [0; 12] => {
                let [a, b, c, d, ..] = self.bytes;
                IpAddress::new_ipv4([a, b, c, d])
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

/// Box and unbox the objects the ABI hands out. A `registered` object also lists itself in
/// the registry of the process that opened it, so the host closes it if the process ends
/// without doing so. A network capability is granted by the host and owns nothing to close.
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
    ($name:ty, registered, $close:ident) => {
        impl $name {
            pub fn into_raw(self) -> *mut Self {
                registry::register(self)
            }
            /// Reclaim an owned object exactly once.
            /// # Safety
            /// Pointer must be live and returned by `into_raw`.
            pub unsafe fn from_raw(raw: *mut Self) -> Self {
                unsafe { registry::reclaim(raw) }
            }
        }
        impl Handle for $name {
            unsafe fn close(raw: *mut Self) {
                // SAFETY: the registry holds only live objects that came from `into_raw`.
                let _ = unsafe { $close(raw) };
            }
        }
    };
}
ownership!(Network);
ownership!(Tcp, registered, xila_tcp_close);
ownership!(TcpListener, registered, xila_tcp_listener_close);
ownership!(Udp, registered, xila_udp_close);

fn status(result: Result<(), Status>) -> Status {
    result.map_or_else(|e| e, |()| Status::Ok)
}
unsafe fn input<'a, T>(p: *const T) -> Result<&'a T, Status> {
    unsafe { p.as_ref() }.ok_or(Status::InvalidArgument)
}
unsafe fn object<'a, T>(p: *mut T) -> Result<&'a mut T, Status> {
    unsafe { p.as_mut() }.ok_or(Status::InvalidArgument)
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
/// The local endpoint a listener or a UDP socket binds. The unspecified address of
/// either family is the wildcard, as in POSIX: smoltcp matches only `None` against
/// every local address, and would never accept on `0.0.0.0` or `::`. `None` matches
/// both families, so the listener and the UDP calls drop traffic of the other family
/// themselves (see [`same_family`]): a wildcard bind keeps its family.
fn listen_endpoint(endpoint: IpEndpoint) -> IpListenEndpoint {
    let address = (!endpoint.address.into_smoltcp().is_unspecified()).then_some(endpoint.address);
    IpListenEndpoint::new(address, endpoint.port)
}

/// Whether smoltcp's address is of the ABI family `family`.
fn same_family(family: u32, address: &smoltcp::wire::IpAddress) -> bool {
    let theirs = match address {
        smoltcp::wire::IpAddress::Ipv4(_) => XILA_ADDRESS_IPV4,
        smoltcp::wire::IpAddress::Ipv6(_) => XILA_ADDRESS_IPV6,
    };
    theirs == family
}

/// Whether the listener socket holds a connection from the other family than it was
/// bound with, which a wildcard listener refuses so that it keeps its family.
fn foreign_connection(socket: &smoltcp::socket::tcp::Socket, address: NetworkAddress) -> bool {
    use smoltcp::socket::tcp::State;
    matches!(socket.state(), State::Established | State::CloseWait)
        && socket
            .remote_endpoint()
            .is_some_and(|remote| !same_family(address.family, &remote.addr))
}

/// Listen on the listener's address again, after a connection was refused.
fn listen_again(socket: &mut smoltcp::socket::tcp::Socket, address: NetworkAddress) {
    if let Ok(endpoint) = address.endpoint() {
        let _ = socket.listen(listen_endpoint(endpoint).into_smoltcp());
    }
}

/// Upper bound on waiting for the stack to send the reset that refuses a cross-family
/// connection. The stack may never send it: on Ethernet or Wi-Fi the peer's link-layer
/// address can be gone, and smoltcp retries neighbor discovery forever. When the limit
/// fires, the listener goes back to listening and the peer sees the refusal only at its
/// next send, as before.
const RESET_TIMEOUT_MS: u64 = 1000;
/// The same limit, in Embassy-time ticks, for the deadline a poll stores on a listener.
const RESET_TIMEOUT_TICKS: u64 = embassy_time::Duration::from_millis(RESET_TIMEOUT_MS).as_ticks();

/// A listener that has a connection from the other family than it was bound with resets
/// it, so a wildcard listener keeps its family and the peer is refused at once. Also
/// waits out a reset that a poll left pending on the closed socket, because `accept`
/// re-listens on a closed socket and would drop it. Returns whether it did.
///
/// The stack sends the reset on its next poll, so it is waited for before listening
/// again: listening before the reset went out would forget the connection it is sent
/// on, leaving the peer to see the refusal only once it sends data itself. The wait is
/// bounded by [`RESET_TIMEOUT_MS`], and the listener listens again whatever the wait
/// ended on.
async fn refuse_other_family(socket: &mut TcpSocket, address: NetworkAddress) -> bool {
    // The check and the abort, in one closure.
    let refuse = socket
        .with_mutable(|s| {
            if foreign_connection(s, address) {
                s.abort();
                true
            } else {
                // A poll may have aborted a connection and left its reset pending.
                use smoltcp::socket::tcp::State;
                s.state() == State::Closed && s.remote_endpoint().is_some()
            }
        })
        .await;
    if !refuse {
        return false;
    }
    // `flush` completes once the pending reset has been sent; race it against the
    // limit so a reset the stack cannot send does not hang the listener.
    let _ = embassy_futures::select::select(
        socket.flush(),
        embassy_time::Timer::after_millis(RESET_TIMEOUT_MS),
    )
    .await;
    listen_again_socket(socket, address).await;
    true
}

/// [`listen_again`] through the socket's stack lock.
async fn listen_again_socket(socket: &mut TcpSocket, address: NetworkAddress) {
    socket.with_mutable(|s| listen_again(s, address)).await;
}

/// The poll form of [`refuse_other_family`]: a poll cannot wait for the reset, so the
/// socket is aborted, the task is woken once the reset has been sent, and the listener
/// then listens again. The deadline stored on the listener bounds that wait: a later
/// poll listens again after it even if the reset is still pending, and the peer then
/// sees the refusal only at its next send, as before. Returns whether the connection
/// was foreign.
fn refuse_other_family_poll(
    socket: &mut smoltcp::socket::tcp::Socket,
    listener: &TcpListener,
    cx: &mut core::task::Context<'_>,
) -> bool {
    use smoltcp::socket::tcp::State;
    if foreign_connection(socket, listener.address) {
        socket.abort();
        // SAFETY: the closure does not lock the deadline again, so no aliasing.
        unsafe {
            listener.reset_deadline.lock_mut(|deadline| {
                *deadline = embassy_time::Instant::now().as_ticks() + RESET_TIMEOUT_TICKS;
            })
        };
        socket.register_send_waker(cx.waker());
        true
    } else if socket.state() == State::Closed {
        // The reset went out, or a waiting connection was reset by its peer, or the
        // reset is still pending: listen again once it went out or its deadline
        // fired, so the listener keeps its family. Poll reports nothing instead of
        // XILA_POLL_ERR meanwhile, which is closer to POSIX.
        if socket.remote_endpoint().is_none() {
            listen_again(socket, listener.address);
        } else {
            let deadline = listener.reset_deadline.lock(|deadline| *deadline);
            if embassy_time::Instant::now().as_ticks() >= deadline {
                // The stack could not send the reset within the limit; listen again
                // and let the peer see the refusal at its next send, as before.
                listen_again(socket, listener.address);
            } else {
                // Wake this poll at the deadline, to listen again even if the reset
                // is still pending then.
                let timer = embassy_time::Timer::at(embassy_time::Instant::from_ticks(deadline));
                let _ = core::pin::pin!(timer).poll(cx);
            }
        }
        false
    } else {
        false
    }
}

// Small fixed socket buffers; no ABI-side staging buffer on I/O paths.
const BUFFER_SIZE: usize = 1024;

/// A new socket, which the caller owns as soon as the future completes.
async fn new_tcp_socket(network: &Network) -> Result<TcpSocket, Status> {
    network
        .manager
        .new_tcp_socket(BUFFER_SIZE, BUFFER_SIZE, None)
        .await
        .map_err(Status::from)
}

/// A new socket, listening on `address`. The socket lives only inside the future until it
/// completes, so a process cancelled while it waits drops the future and the socket with it.
async fn listening_socket(network: &Network, address: NetworkAddress) -> Result<TcpSocket, Status> {
    let endpoint = address.endpoint()?;
    let mut socket = new_tcp_socket(network).await?;
    let listening = socket
        .with_mutable(|s| s.listen(listen_endpoint(endpoint).into_smoltcp()))
        .await;
    if listening.is_err() {
        socket.close_forced().await;
        return Err(Status::InvalidArgument);
    }
    Ok(socket)
}

/// Release network authority; existing sockets remain valid. Null succeeds.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
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
pub unsafe extern "C" fn xila_tcp_connect(
    network: *const Network,
    address: *const NetworkAddress,
    timeout_ms: u32,
    connection: *mut *mut Tcp,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if connection.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let network = input(network)?;
        let endpoint = input(address)?.endpoint()?;
        network.require(XILA_NETWORK_CONNECT)?;
        // The socket exists only inside the future until the connection is made, so a
        // process cancelled while it connects drops the future and the socket with it.
        let connected = suspend_on_cancellable(async {
            let mut inner = new_tcp_socket(network).await?;
            let result = if timeout_ms == 0 {
                inner.connect(endpoint.address, endpoint.port).await
            } else {
                use embassy_futures::select::{Either, select};
                let start = time::MonotonicClock::now(&time::EmbassyClock);
                match select(
                    inner.connect(endpoint.address, endpoint.port),
                    // A plain deadline check: embassy-time timers need an Embassy executor
                    // waker, which the non-coroutine `block_on` fallback does not provide.
                    // Re-waking yields cleanly inside a coroutine and keeps the deadline
                    // live under executors that sleep until woken.
                    core::future::poll_fn(|context| {
                        if time::MonotonicClock::now(&time::EmbassyClock).saturating_sub(start)
                            >= core::time::Duration::from_millis(u64::from(timeout_ms))
                        {
                            core::task::Poll::Ready(())
                        } else {
                            context.waker().wake_by_ref();
                            core::task::Poll::Pending
                        }
                    }),
                )
                .await
                {
                    Either::First(result) => result,
                    Either::Second(_) => Err(::network::IoErrorKind::TimedOut),
                }
            };
            if let Err(error) = result {
                inner.close_forced().await;
                return Err(Status::from(error));
            }
            Ok::<_, Status>(inner)
        });
        Ok(Tcp { inner: connected? }.into_raw())
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { connection.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Bind and start a single-slot TCP listener. Port zero is rejected by Core.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_listen(
    network: *const Network,
    address: *const NetworkAddress,
    listener: *mut *mut TcpListener,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if listener.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let network = input(network)?;
        let address = *input(address)?;
        address.endpoint()?;
        network.require(XILA_NETWORK_LISTEN)?;
        let inner = suspend_on_cancellable(listening_socket(network, address))?;
        Ok(TcpListener {
            network: Network::new(network.manager, u32::from(network.rights))?,
            address,
            inner,
            reset_deadline: synchronization::blocking_mutex::Mutex::new(0),
        }
        .into_raw())
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { listener.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Wait for a connection and replenish the listener. Run the interface runner independently.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_accept(
    listener: *mut TcpListener,
    connection: *mut *mut Tcp,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if connection.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let listener = object(listener)?;
        listener.network.require(XILA_NETWORK_LISTEN)?;
        let endpoint = listen_endpoint(listener.address.endpoint()?);
        let address = listener.address;
        loop {
            // A poll may have aborted a connection and left its reset pending: wait it
            // out and listen again before accepting, because `accept` re-listens on a
            // closed socket and would drop the pending reset.
            if suspend_on(refuse_other_family(&mut listener.inner, address)) {
                continue;
            }
            suspend_on(
                listener
                    .inner
                    .accept(endpoint.address.clone(), endpoint.port),
            )
            .map_err(Status::from)?;
            // A wildcard listener takes connections of its own family only.
            if !suspend_on(refuse_other_family(&mut listener.inner, address)) {
                break;
            }
        }
        if suspend_on(listener.inner.get_remote_endpoint())
            .map_err(Status::from)?
            .is_none()
        {
            return Err(Status::ConnectionReset);
        }
        let replacement =
            suspend_on_cancellable(listening_socket(&listener.network, listener.address))?;
        let inner = core::mem::replace(&mut listener.inner, replacement);
        Ok(Tcp { inner }.into_raw())
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { connection.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Abort and release the socket and its buffers. Null succeeds; queued data may be lost.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_close(socket: *mut Tcp) -> Status {
    status(unsafe {
        if !socket.is_null() {
            // Closed while still registered: a process cancelled in the middle of this
            // call leaves the socket to the host, which aborts it again.
            suspend_on((*socket).inner.close_forced());
            drop(Tcp::from_raw(socket));
        }
        Ok(())
    })
}

/// Abort and release the socket and its buffers. Null succeeds; queued data may be lost.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_listener_close(socket: *mut TcpListener) -> Status {
    status(unsafe {
        if !socket.is_null() {
            // Closed while still registered, as for a connection.
            suspend_on((*socket).inner.close_forced());
            drop(TcpListener::from_raw(socket));
        }
        Ok(())
    })
}

/// Gracefully shut down writing. Only XILA_SHUTDOWN_WRITE is supported; reads remain valid.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_shutdown(socket: *mut Tcp, direction: u32) -> Status {
    status((|| unsafe {
        let socket = object(socket)?;
        if direction != XILA_SHUTDOWN_WRITE {
            return Err(Status::InvalidArgument);
        }
        suspend_on(socket.inner.close());
        Ok(())
    })())
}

/// Blocking direct-buffer I/O. Zero size permits null; send may be partial; receive zero means EOF.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_send(
    socket: *mut Tcp,
    buffer: *const c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if count.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let socket = object(socket)?;
        let buffer = read_buffer(buffer, size)?;
        let n = if size == 0 {
            0
        } else {
            suspend_on(socket.inner.write(buffer)).map_err(Status::from)?
        };
        Ok(n)
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { count.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Blocking direct-buffer I/O. Zero size permits null; send may be partial; receive zero means EOF.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_receive(
    socket: *mut Tcp,
    buffer: *mut c_void,
    size: usize,
    count: *mut usize,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if count.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let socket = object(socket)?;
        let buffer = write_buffer(buffer, size)?;
        let n = if size == 0 {
            0
        } else {
            suspend_on(socket.inner.read(buffer)).map_err(Status::from)?
        };
        Ok(n)
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { count.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Read the connection endpoint.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_local_address(
    socket: *const Tcp,
    address: *mut NetworkAddress,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if address.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let socket = input(socket)?;
        let (ip, port) = suspend_on(socket.inner.get_local_endpoint())
            .map_err(Status::from)?
            .ok_or(Status::NotConnected)?;
        Ok(NetworkAddress::new(ip, port))
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { address.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Read the connection endpoint.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_tcp_peer_address(
    socket: *const Tcp,
    address: *mut NetworkAddress,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if address.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let socket = input(socket)?;
        let (ip, port) = suspend_on(socket.inner.get_remote_endpoint())
            .map_err(Status::from)?
            .ok_or(Status::NotConnected)?;
        Ok(NetworkAddress::new(ip, port))
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { address.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Bind a UDP socket. Port zero is rejected by Core; one datagram per direction is buffered.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_udp_bind(
    network: *const Network,
    address: *const NetworkAddress,
    socket: *mut *mut Udp,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if socket.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let network = input(network)?;
        let address = input(address)?;
        let address_family = address.family;
        let endpoint = address.endpoint()?;
        network.require(XILA_NETWORK_LISTEN)?;
        // The socket exists only inside the future until it is bound, as for a connection.
        let bound = suspend_on_cancellable(async {
            let mut inner = network
                .manager
                .new_udp_socket(BUFFER_SIZE, BUFFER_SIZE, 1, 1, None)
                .await
                .map_err(Status::from)?;
            let result = inner
                .with_mutable(|s| s.bind(listen_endpoint(endpoint).into_smoltcp()))
                .await;
            if result.is_err() {
                inner.close().await;
                return Err(Status::InvalidArgument);
            }
            Ok::<_, Status>(inner)
        });
        Ok(Udp {
            inner: bound?,
            rights: network.rights,
            family: address_family,
        }
        .into_raw())
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { socket.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Release a UDP socket. Null succeeds.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_udp_close(socket: *mut Udp) -> Status {
    status(unsafe {
        if !socket.is_null() {
            // Closed while still registered, as for TCP: a process cancelled in the middle
            // of this call leaves the socket to the host, which closes it again.
            suspend_on((*socket).inner.close());
            drop(Udp::from_raw(socket));
        }
        Ok(())
    })
}

/// Nonblocking atomic datagram send, without logging or staging allocation.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_udp_send_to(
    socket: *mut Udp,
    buffer: *const c_void,
    size: usize,
    address: *const NetworkAddress,
    count: *mut usize,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if count.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let socket = object(socket)?;
        let destination = input(address)?;
        let endpoint = destination.endpoint()?;
        let buffer = read_buffer(buffer, size)?;
        if u32::from(socket.rights) & XILA_NETWORK_CONNECT == 0 {
            return Err(Status::PermissionDenied);
        }
        if destination.family != socket.family {
            return Err(Status::AddressFamilyUnsupported);
        }
        suspend_on(socket.inner.with_mutable(|s| {
            if size > s.payload_send_capacity() {
                return Err(Status::Overflow);
            }
            s.send_slice(buffer, endpoint.into_smoltcp())
                .map_err(|e| match e {
                    smoltcp::socket::udp::SendError::BufferFull => Status::WouldBlock,
                    smoltcp::socket::udp::SendError::Unaddressable => Status::HostUnreachable,
                })
        }))?;
        Ok(size)
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { count.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Nonblocking datagram receive. Overflow preserves the datagram; outputs change only on success.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_udp_receive_from(
    socket: *mut Udp,
    buffer: *mut c_void,
    size: usize,
    address: *mut NetworkAddress,
    count: *mut usize,
) -> Status {
    // Checked here, outside the closure, so the null guards are visible to analysis.
    if count.is_null() || address.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| {
        unsafe {
            let socket = object(socket)?;
            let buffer = write_buffer(buffer, size)?;
            let family = socket.family;
            let (n, endpoint) = suspend_on(socket.inner.with_mutable(|s| {
                drop_other_family(s, family);
                // Peek first so overflow preserves the datagram and caller buffer.
                let (data, _) = s.peek().map_err(|_| Status::WouldBlock)?;
                if data.len() > size {
                    return Err(Status::Overflow);
                }
                s.recv_slice(buffer).map_err(|_| Status::InputOutput)
            }))?;
            Ok((
                n,
                NetworkAddress::new(
                    IpAddress::from_smoltcp(&endpoint.endpoint.addr),
                    Port::new(endpoint.endpoint.port),
                ),
            ))
        }
    })();
    match result {
        Ok((n, endpoint)) => {
            // SAFETY: both outputs were checked non-null at the top of this function.
            unsafe {
                address.write(endpoint);
                count.write(n);
            }
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Resolve one address family synchronously. Count is capacity in/results out; capacity zero queries size. Overflow leaves outputs unchanged. DNS allocates inside Core.
/// # Safety
/// Objects must be live and calls serialized. Inputs must be readable and outputs
/// aligned, writable and disjoint from inputs and objects. Buffers must be valid
/// for their lengths. Owned objects must be released exactly once.
pub unsafe extern "C" fn xila_network_resolve(
    network: *const Network,
    host: *const c_char,
    host_length: usize,
    family: u32,
    addresses: *mut NetworkAddress,
    count: *mut usize,
) -> Status {
    // Checked here, outside the closure, so the null guard is visible to analysis.
    if count.is_null() {
        return Status::InvalidArgument;
    }
    let result = (|| unsafe {
        let network = input(network)?;
        let capacity = *count;
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
            suspend_on(network.manager.resolve(host, kind, false, None)).map_err(Status::from)?;
        let matches = |ip: &&IpAddress| {
            matches!(
                (family, ip),
                (XILA_ADDRESS_IPV4, IpAddress::IPv4(_)) | (XILA_ADDRESS_IPV6, IpAddress::IPv6(_))
            )
        };
        let needed = resolved.iter().filter(matches).count();
        if capacity == 0 {
            return Ok(needed);
        }
        if needed > capacity {
            return Err(Status::Overflow);
        }
        for (index, ip) in resolved.iter().filter(matches).enumerate() {
            addresses
                .add(index)
                .write(NetworkAddress::new(ip.clone(), Port::new(0)));
        }
        Ok(needed)
    })();
    match result {
        Ok(value) => {
            // SAFETY: checked non-null at the top of this function.
            unsafe { count.write(value) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Drop the queued datagrams of the other family than the socket was bound with.
fn drop_other_family(socket: &mut smoltcp::socket::udp::Socket, family: u32) {
    while let Ok((_, metadata)) = socket.peek() {
        if same_family(family, &metadata.endpoint.addr) {
            break;
        }
        let _ = socket.recv();
    }
}

pub(crate) fn tcp_readiness(
    connection: &TcpSocket,
    cx: &mut core::task::Context<'_>,
    listener: Option<&TcpListener>,
    events: u16,
) -> u16 {
    use crate::{XILA_POLL_ERR, XILA_POLL_HUP, XILA_POLL_IN, XILA_POLL_OUT};
    use core::task::Poll;
    use smoltcp::socket::tcp::State;

    // A listener refuses a connection of the other family before it reports one, so
    // `poll` and a nonblocking accept never see it. The reset is sent by the stack on
    // its next poll; the task is woken then, and the listener listens again.
    if let Some(listener) = listener {
        let _ = connection.poll_with_mutable(cx, |socket, cx| {
            Poll::Ready(refuse_other_family_poll(socket, listener, cx))
        });
    }
    let listener = listener.is_some();

    let result = connection.poll_with(cx, |socket, _| {
        let state = socket.state();
        let mut ready = 0;
        if listener {
            if matches!(state, State::Established | State::CloseWait) {
                ready |= events & XILA_POLL_IN;
            } else if state == State::Closed && socket.remote_endpoint().is_none() {
                // A closed listener still attached to a connection is being reset, or
                // a waiting connection was reset by its peer: report nothing until the
                // reset has been sent and it listens again, which is closer to POSIX.
                ready |= XILA_POLL_ERR;
            }
        } else {
            if socket.can_recv() {
                ready |= events & XILA_POLL_IN;
            }
            if socket.can_send() {
                ready |= events & XILA_POLL_OUT;
            }
            if matches!(
                state,
                State::CloseWait
                    | State::Closing
                    | State::LastAck
                    | State::TimeWait
                    | State::Closed
            ) {
                ready |= XILA_POLL_HUP;
            }
        }
        Poll::Ready((ready, state))
    });
    match result {
        Poll::Ready((ready, State::Closed)) if !listener => {
            // A fully closed socket can represent either a FIN or a reset.
            // Peeking zero bytes asks smoltcp which one without consuming data.
            match connection.poll_with_mutable(cx, |socket, _| {
                let reset = matches!(
                    socket.peek(0),
                    Err(smoltcp::socket::tcp::RecvError::InvalidState)
                );
                Poll::Ready(reset)
            }) {
                Poll::Ready(true) => ready | XILA_POLL_ERR,
                Poll::Ready(false) => ready,
                Poll::Pending => 0,
            }
        }
        Poll::Ready((ready, _)) => ready,
        Poll::Pending => 0,
    }
}

pub(crate) fn udp_readiness(socket: &Udp, cx: &mut core::task::Context<'_>, events: u16) -> u16 {
    use crate::{XILA_POLL_ERR, XILA_POLL_IN, XILA_POLL_OUT};
    use core::task::Poll;

    let family = socket.family;
    let result = socket.inner.poll_with_mutable(cx, |inner, _| {
        drop_other_family(inner, family);
        if !inner.is_open() {
            return Poll::Ready(XILA_POLL_ERR);
        }
        let mut ready = 0;
        if inner.can_recv() {
            ready |= events & XILA_POLL_IN;
        }
        if inner.can_send() && u32::from(socket.rights) & XILA_NETWORK_CONNECT != 0 {
            ready |= events & XILA_POLL_OUT;
        }
        Poll::Ready(ready)
    });
    match result {
        Poll::Ready(ready) => ready,
        Poll::Pending => 0,
    }
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
    fn unspecified_address_listens_everywhere() {
        let port = Port::new(7000);
        for unspecified in [
            IpAddress::new_ipv4([0, 0, 0, 0]),
            IpAddress::new_ipv6([0; 8]),
        ] {
            let endpoint = listen_endpoint(IpEndpoint::new(unspecified, port));
            assert_eq!(endpoint.address, None);
            assert_eq!(endpoint.port, port);
        }
        let loopback = IpAddress::new_ipv4([127, 0, 0, 1]);
        assert_eq!(
            listen_endpoint(IpEndpoint::new(loopback.clone(), port)).address,
            Some(loopback)
        );
    }
    #[test]
    fn wildcard_binds_keep_their_family() {
        let v4 = IpAddress::new_ipv4([127, 0, 0, 1]).into_smoltcp();
        let v6 = IpAddress::new_ipv6([0, 0, 0, 0, 0, 0, 0, 1]).into_smoltcp();
        assert!(same_family(XILA_ADDRESS_IPV4, &v4));
        assert!(!same_family(XILA_ADDRESS_IPV4, &v6));
        assert!(same_family(XILA_ADDRESS_IPV6, &v6));
        assert!(!same_family(XILA_ADDRESS_IPV6, &v4));
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
        assert_eq!(
            Status::from(::network::IoErrorKind::TimedOut),
            Status::TimedOut
        );
        assert_eq!(
            Status::from(::network::IoErrorKind::ResourceBusy),
            Status::ResourceBusy
        );
    }
}
