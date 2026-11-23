use crate::{
    DnsQueryKind, IoErrorKind, IpAddress, IpCidr, Ipv4, Ipv6, MacAddress, Port, ResolveError,
    ResolveResult, Result, Route, WakeSignal,
};
use alloc::{boxed::Box, string::String, vec, vec::Vec};
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use file_system::DirectCharacterDevice;
use shared::poll_pin_ready;
use smoltcp::{
    config::{DNS_MAX_SERVER_COUNT, IFACE_MAX_ADDR_COUNT},
    iface::{self, SocketHandle, SocketSet},
    phy::{Device, Medium},
    socket::{AnySocket, Socket, dhcpv4, dns},
    wire::{self, DnsQueryType, EthernetAddress, Ipv4Address, Ipv4Cidr},
};
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

/// What a DHCP lease added to the stack, so that exactly that can be removed
/// again. Entries that were already present, such as static ones, are never
/// recorded and are never touched.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DhcpLease {
    address: Ipv4Cidr,
    router: Option<Ipv4Address>,
    dns_servers: Vec<Ipv4Address>,
    added_address: bool,
    added_route: bool,
    added_dns_servers: Vec<Ipv4Address>,
}

pub struct StackInner {
    pub name: String,
    pub is_up: bool,
    pub enabled: bool,
    pub interface: smoltcp::iface::Interface,
    pub controller: Box<dyn DirectCharacterDevice>,
    pub sockets: smoltcp::iface::SocketSet<'static>,
    pub dhcp_socket: Option<smoltcp::iface::SocketHandle>,
    pub dns_socket: Option<SocketHandle>,
    pub dns_servers: Vec<smoltcp::wire::IpAddress>,
    dhcp_lease: Option<DhcpLease>,
    pub maximum_transmission_unit: usize,
    pub maximum_burst_size: Option<usize>,
    pub next_local_port: Port,
    time_manager: &'static time::Manager<'static>,
    retired_sockets: Vec<(SocketHandle, smoltcp::time::Instant)>,
}

#[derive(Clone)]
pub struct Stack {
    inner: Arc<Mutex<CriticalSectionRawMutex, StackInner>>,
    wake_signal: WakeSignal,
    time_manager: &'static time::Manager<'static>,
}

impl Stack {
    pub fn new(inner: StackInner, wake_signal: WakeSignal) -> Self {
        let time_manager = inner.time_manager;
        Stack {
            inner: Arc::new(Mutex::new(inner)),
            wake_signal,
            time_manager,
        }
    }

    pub fn wake_up(&self) -> &WakeSignal {
        &self.wake_signal
    }

    pub fn time_manager(&self) -> &'static time::Manager<'static> {
        self.time_manager
    }

    /// Try to lock the inner stack without blocking. Returns None if the lock is held.
    pub fn try_lock(
        &self,
    ) -> Option<synchronization::mutex::MutexGuard<'_, CriticalSectionRawMutex, StackInner>> {
        self.inner.try_lock().ok()
    }

    /// Lock the inner stack asynchronously.
    pub async fn lock(
        &self,
    ) -> synchronization::mutex::MutexGuard<'_, CriticalSectionRawMutex, StackInner> {
        self.inner.lock().await
    }

    /// Signal the runner to wake up (call after modifying the stack outside of with_mutable).
    pub fn wake_runner(&self) {
        self.wake_signal.signal(());
    }

    pub async fn with<R, F: FnOnce(&StackInner) -> R>(&self, f: F) -> R {
        let stack = self.inner.lock().await;
        f(&stack)
    }

    pub async fn with_mutable<R, F: FnOnce(&mut StackInner) -> R>(&self, f: F) -> R {
        let mut stack = self.inner.lock().await;
        let r = f(&mut stack);
        // Wake the runner via signal
        self.wake_signal.signal(());
        r
    }

    pub async fn with_mutable_no_wake<R, F: FnOnce(&mut StackInner) -> R>(&self, f: F) -> R {
        let mut stack = self.inner.lock().await;
        f(&mut stack)
    }

    pub fn poll_with<R, F: FnOnce(&StackInner, &mut Context<'_>) -> Poll<R>>(
        &self,
        context: &mut Context<'_>,
        f: F,
    ) -> Poll<R> {
        let stack = poll_pin_ready!(self.inner.lock(), context);
        f(&stack, context)
    }

    fn poll_with_mutable_impl<R, F: FnOnce(&mut StackInner, &mut Context<'_>) -> Poll<R>>(
        &self,
        context: &mut core::task::Context<'_>,
        f: F,
        wake: bool,
    ) -> Poll<R> {
        let mut stack = poll_pin_ready!(self.inner.lock(), context);

        let r = f(&mut stack, context);
        if wake {
            // Wake the runner via signal
            self.wake_signal.signal(());
        }
        r
    }

    pub fn poll_with_mutable<R, F: FnOnce(&mut StackInner, &mut Context<'_>) -> Poll<R>>(
        &self,
        context: &mut core::task::Context<'_>,
        f: F,
    ) -> Poll<R> {
        self.poll_with_mutable_impl(context, f, true)
    }
}

impl StackInner {
    pub fn new(
        name: impl AsRef<str>,
        device: &mut (impl Device + 'static),
        controller_device: impl DirectCharacterDevice + 'static,
        random_seed: u64,
        now: smoltcp::time::Instant,
        time_manager: &'static time::Manager<'static>,
    ) -> Self {
        let capabilities = device.capabilities();

        let mut config = match capabilities.medium {
            Medium::Ethernet => {
                iface::Config::new(EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]).into())
            }
            Medium::Ip => iface::Config::new(smoltcp::wire::HardwareAddress::Ip),
            // Another crate of the build may enable a smoltcp medium this
            // subsystem does not drive, such as IEEE 802.15.4.
            #[allow(unreachable_patterns)]
            _ => unimplemented!("unsupported network medium"),
        };
        config.random_seed = random_seed;

        let interface = smoltcp::iface::Interface::new(config, device, now);

        let sockets = SocketSet::new(vec![]);

        let next_local_port = (random_seed
            % (Port::MAXIMUM.into_inner() - Port::MINIMUM_USER.into_inner()) as u64)
            as u16
            + Port::MINIMUM_USER.into_inner();

        StackInner {
            name: name.as_ref().into(),
            is_up: true,
            enabled: true,
            interface,
            controller: Box::new(controller_device),
            sockets,
            retired_sockets: Vec::new(),
            dhcp_socket: None,
            dns_socket: None,
            dns_servers: Vec::with_capacity(DNS_MAX_SERVER_COUNT),
            dhcp_lease: None,
            maximum_transmission_unit: capabilities.max_transmission_unit,
            maximum_burst_size: capabilities.max_burst_size,
            next_local_port: Port::from_inner(next_local_port),
            time_manager,
        }
    }

    pub fn is_available(&self) -> bool {
        self.enabled && self.is_link_up()
    }

    pub fn is_link_up(&self) -> bool {
        controller_link_state(self.controller.as_ref(), self.is_up)
    }

    pub fn get_state(&self) -> bool {
        self.enabled
    }

    pub fn set_state(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn get_route(&mut self, index: usize) -> Option<Route> {
        let mut route = None;

        self.interface.routes_mut().update(|v| {
            if let Some(r) = v.get(index) {
                route = Some(*r);
            }
        });

        route.map(Route::from_smoltcp)
    }

    pub fn get_route_count(&mut self) -> usize {
        let mut count = 0;

        self.interface.routes_mut().update(|v| count = v.len());

        count
    }

    pub fn add_route(&mut self, route: Route) -> Result<()> {
        let mut result = Ok(());

        self.interface.routes_mut().update(|v| {
            result = v
                .push(route.into_smoltcp())
                .map_err(|_| IoErrorKind::NoBuffers)
        });

        result
    }

    pub fn remove_route(&mut self, index: usize) -> Option<Route> {
        let mut route = None;

        self.interface.routes_mut().update(|v| {
            if index < v.len() {
                route = Some(v.remove(index));
            }
        });

        route.map(Route::from_smoltcp)
    }

    pub fn get_dns_servers_count(&self) -> usize {
        self.dns_servers.len()
    }

    pub fn get_dns_server(&self, index: usize) -> Option<IpAddress> {
        self.dns_servers.get(index).map(IpAddress::from_smoltcp)
    }

    pub fn add_dns_server(&mut self, server: IpAddress) -> Result<()> {
        if self.dns_servers.len() >= DNS_MAX_SERVER_COUNT {
            return Err(IoErrorKind::NoBuffers);
        }

        self.dns_servers.push(server.into_smoltcp());
        self.update_socket_dns_servers();
        Ok(())
    }

    pub fn remove_dns_server(&mut self, index: usize) -> Option<IpAddress> {
        if index >= self.dns_servers.len() {
            return None;
        }

        let server = self.dns_servers.remove(index);
        self.update_socket_dns_servers();
        Some(IpAddress::from_smoltcp(&server))
    }

    fn update_socket_dns_servers(&mut self) {
        if let Some(handle) = self.dns_socket {
            let socket = self.sockets.get_mut::<dns::Socket>(handle);
            socket.update_servers(&self.dns_servers);
        }
    }

    pub fn get_dns_servers(&self) -> &[wire::IpAddress] {
        &self.dns_servers
    }

    pub fn set_dhcp_state(&mut self, enabled: bool) {
        if enabled {
            if self.dhcp_socket.is_none() {
                let handle = self.sockets.add(dhcpv4::Socket::new());
                self.dhcp_socket = Some(handle);
            }
        } else if let Some(handle) = self.dhcp_socket.take() {
            self.sockets.remove(handle);
            self.remove_dhcp_lease();
        }
    }

    /// Apply a configured DHCP lease, replacing the previous one if it differs.
    ///
    /// The address is added with its prefix, the default route goes through the
    /// router and the DNS servers join the list the DNS socket resolves with.
    /// Anything the stack already holds (for example a static address) is kept
    /// as it is and is not recorded as part of the lease.
    fn apply_dhcp_lease(
        &mut self,
        address: Ipv4Cidr,
        router: Option<Ipv4Address>,
        dns_servers: Vec<Ipv4Address>,
    ) {
        if let Some(lease) = &self.dhcp_lease
            && lease.address == address
            && lease.router == router
            && lease.dns_servers == dns_servers
        {
            return;
        }

        self.remove_dhcp_lease();

        let mut lease = DhcpLease {
            address,
            router,
            dns_servers,
            added_address: false,
            added_route: false,
            added_dns_servers: Vec::new(),
        };

        let wire_address = wire::IpCidr::Ipv4(address);
        if self.interface.ip_addrs().contains(&wire_address) {
            // Already configured statically: keep it and do not claim it.
        } else {
            self.interface.update_ip_addrs(|addresses| {
                lease.added_address = addresses.push(wire_address).is_ok();
            });
            if !lease.added_address {
                // Without its address the lease is useless, and a route or DNS
                // server of another subnet would only misroute traffic. The
                // lease is not recorded, so an identical lease is applied again
                // once an address slot is free.
                log::warning!(
                    "DHCP lease not applied, the address table is full, address {}",
                    address.address()
                );
                return;
            }
        }

        if let Some(router) = router
            && self.interface.routes().get_default_ipv4_route().is_none()
        {
            let route = smoltcp::iface::Route::new_ipv4_gateway(router);
            self.interface.routes_mut().update(|routes| {
                lease.added_route = routes.push(route).is_ok();
            });
            if !lease.added_route {
                log::warning!("DHCP default route not applied, the route table is full");
            }
        }

        for server in lease.dns_servers.clone() {
            let server = wire::IpAddress::Ipv4(server);
            if self.dns_servers.contains(&server) {
                continue;
            }
            if self.dns_servers.len() >= DNS_MAX_SERVER_COUNT {
                log::warning!("DHCP DNS server not applied, the DNS server list is full");
                break;
            }
            self.dns_servers.push(server);
            if let wire::IpAddress::Ipv4(server) = server {
                lease.added_dns_servers.push(server);
            }
        }
        self.update_socket_dns_servers();

        log::information!(
            "DHCP lease acquired, address {}, prefix {}, router {}, DNS servers {}",
            address.address(),
            address.prefix_len(),
            router.map_or_else(|| String::from("none"), |router| alloc::format!("{router}")),
            lease.added_dns_servers.len()
        );

        self.dhcp_lease = Some(lease);
    }

    /// Remove exactly what the current DHCP lease added.
    fn remove_dhcp_lease(&mut self) {
        let Some(lease) = self.dhcp_lease.take() else {
            return;
        };

        if lease.added_address {
            let wire_address = wire::IpCidr::Ipv4(lease.address);
            self.interface.update_ip_addrs(|addresses| {
                if let Some(index) = addresses.iter().position(|a| *a == wire_address) {
                    addresses.remove(index);
                }
            });
        }

        if lease.added_route
            && let Some(router) = lease.router
        {
            let route = smoltcp::iface::Route::new_ipv4_gateway(router);
            self.interface.routes_mut().update(|routes| {
                if let Some(index) = routes
                    .iter()
                    .position(|r| r.cidr == route.cidr && r.via_router == route.via_router)
                {
                    routes.remove(index);
                }
            });
        }

        if !lease.added_dns_servers.is_empty() {
            for server in &lease.added_dns_servers {
                let server = wire::IpAddress::Ipv4(*server);
                if let Some(index) = self.dns_servers.iter().position(|s| *s == server) {
                    self.dns_servers.remove(index);
                }
            }
            self.update_socket_dns_servers();
        }

        log::information!("DHCP lease released, address {}", lease.address.address());
    }

    /// Read the DHCP socket's events and apply them. Returns whether the lease
    /// changed.
    fn poll_dhcp(&mut self) -> bool {
        enum Change {
            Configured(Ipv4Cidr, Option<Ipv4Address>, Vec<Ipv4Address>),
            Deconfigured,
        }

        let Some(handle) = self.dhcp_socket else {
            return false;
        };

        let change = match self.sockets.get_mut::<dhcpv4::Socket>(handle).poll() {
            None => return false,
            Some(dhcpv4::Event::Configured(config)) => Change::Configured(
                config.address,
                config.router,
                config.dns_servers.iter().copied().collect(),
            ),
            Some(dhcpv4::Event::Deconfigured) => Change::Deconfigured,
        };

        match change {
            Change::Configured(address, router, dns_servers) => {
                self.apply_dhcp_lease(address, router, dns_servers);
            }
            Change::Deconfigured => self.remove_dhcp_lease(),
        }

        true
    }

    pub fn get_dhcp_state(&self) -> bool {
        self.dhcp_socket.is_some()
    }

    pub fn get_ip_addresses_count(&self) -> usize {
        self.interface.ip_addrs().len()
    }

    pub fn get_ip_address(&self, index: usize) -> Option<IpCidr> {
        self.interface
            .ip_addrs()
            .get(index)
            .map(IpCidr::from_smoltcp)
    }

    pub fn add_ip_address(&mut self, cidr: IpCidr) -> Result<()> {
        if self.get_ip_addresses_count() >= IFACE_MAX_ADDR_COUNT {
            return Err(IoErrorKind::NoBuffers);
        }

        let mut result = Ok(());

        self.interface.update_ip_addrs(|addrs| {
            result = addrs
                .push(cidr.into_smoltcp())
                .map_err(|_| IoErrorKind::NoBuffers);
        });

        result
    }

    pub fn remove_ip_address(&mut self, index: usize) -> Option<IpCidr> {
        let mut cidr = None;

        self.interface.update_ip_addrs(|addrs| {
            if index < addrs.len() {
                cidr = Some(addrs.remove(index));
            }
        });

        cidr.as_ref().map(IpCidr::from_smoltcp)
    }

    pub fn set_hardware_address(&mut self, address: &MacAddress) {
        self.interface
            .set_hardware_addr(smoltcp::wire::HardwareAddress::Ethernet(
                smoltcp::wire::EthernetAddress(*address),
            ));
    }

    pub fn get_hardware_address(&self) -> Option<MacAddress> {
        match self.interface.hardware_addr() {
            smoltcp::wire::HardwareAddress::Ethernet(addr) => Some(MacAddress::from(addr.0)),
            _ => None,
        }
    }

    pub fn get_maximum_transmission_unit(&self) -> usize {
        self.maximum_transmission_unit
    }

    pub fn get_maximum_burst_size(&self) -> Option<usize> {
        self.maximum_burst_size
    }

    pub fn add_socket(&mut self, socket: impl AnySocket<'static>) -> smoltcp::iface::SocketHandle {
        self.sockets.add(socket)
    }

    pub fn remove_socket<'a>(&'a mut self, handle: smoltcp::iface::SocketHandle) -> Socket<'a> {
        self.sockets.remove(handle)
    }

    pub fn get_source_ip_v6_address(&mut self, remote: Ipv6) -> Ipv6 {
        let ip = self
            .interface
            .get_source_address_ipv6(&remote.into_smoltcp());

        Ipv6::from_smoltcp(&ip)
    }

    pub fn get_source_ip_v4_address(&mut self, remote: Ipv4) -> Option<Ipv4> {
        self.interface
            .get_source_address_ipv4(&remote.into_smoltcp())
            .map(|ip| Ipv4::from_smoltcp(&ip))
    }

    /// Release datagram sockets immediately; let TCP dispatch queued data and
    /// FIN/RST before reclaiming its storage. Bound abandoned shutdowns.
    pub(crate) fn retire_socket(&mut self, handle: SocketHandle) {
        let pending_tcp = self.sockets.iter().any(|(h, socket)| {
            h == handle
                && matches!(socket, Socket::Tcp(s) if s.is_open() || s.remote_endpoint().is_some())
        });
        if pending_tcp {
            let deadline =
                crate::get_smoltcp_time(self.time_manager) + smoltcp::time::Duration::from_secs(15);
            self.retired_sockets.push((handle, deadline));
        } else {
            self.sockets.remove(handle);
        }
    }

    pub(crate) fn reap_retired_sockets(&mut self) {
        let now = crate::get_smoltcp_time(self.time_manager);
        self.retired_sockets.retain(|(handle, deadline)| {
            let socket = self.sockets.get::<smoltcp::socket::tcp::Socket>(*handle);
            let finished = (!socket.is_open() && socket.remote_endpoint().is_none())
                || socket.state() == smoltcp::socket::tcp::State::TimeWait;
            if finished || now >= *deadline {
                self.sockets.remove(*handle);
                false
            } else {
                true
            }
        });
    }

    pub fn poll(&mut self, device: &mut impl Device) -> (Option<Duration>, bool) {
        let now = crate::get_smoltcp_time(self.time_manager);

        let mut changed = self.interface.poll(now, device, &mut self.sockets)
            == iface::PollResult::SocketStateChanged;
        changed |= self.poll_dhcp();
        self.reap_retired_sockets();

        let poll_at = self.interface.poll_at(now, &self.sockets);

        (
            poll_at.map(|instant| Duration::from_micros((instant - now).total_micros())),
            changed,
        )
    }

    pub fn get_socket<S: AnySocket<'static>>(
        &mut self,
        handle: smoltcp::iface::SocketHandle,
    ) -> &mut S {
        self.sockets.get_mut::<S>(handle)
    }

    pub fn get_next_port(&mut self) -> Port {
        let port = self.next_local_port;

        self.next_local_port = if port.into_inner() == Port::MAXIMUM.into_inner() {
            Port::MINIMUM_USER
        } else {
            Port::from_inner(port.into_inner() + 1)
        };

        port
    }

    pub fn get_dns_socket_handle(&mut self) -> SocketHandle {
        match self.dns_socket {
            Some(handle) => handle,
            None => {
                let dns_socket = dns::Socket::new(&self.dns_servers, vec![]);
                let handle = self.sockets.add(dns_socket);
                self.dns_socket = Some(handle);
                handle
            }
        }
    }

    pub fn start_dns_query(
        &mut self,
        name: &str,
        kind: DnsQueryKind,
    ) -> ResolveResult<dns::QueryHandle> {
        let query_type = if kind.contains(DnsQueryKind::A) {
            DnsQueryType::A
        } else if kind.contains(DnsQueryKind::Aaaa) {
            DnsQueryType::Aaaa
        } else if kind.contains(DnsQueryKind::Cname) {
            DnsQueryType::Cname
        } else if kind.contains(DnsQueryKind::Ns) {
            DnsQueryType::Ns
        } else if kind.contains(DnsQueryKind::Soa) {
            DnsQueryType::Soa
        } else {
            return Err(ResolveError::UnsupportedQuery);
        };

        let socket_handle = self.get_dns_socket_handle();
        let query_handle = self
            .sockets
            .get_mut::<dns::Socket>(socket_handle)
            .start_query(self.interface.context(), name, query_type)
            .map_err(crate::error::dns_start_error)?;

        Ok(query_handle)
    }

    pub fn get_dns_query_result(
        &mut self,
        query_handle: dns::QueryHandle,
        waker: Option<&Waker>,
    ) -> Poll<ResolveResult<Vec<IpAddress>>> {
        let socket_handle = self.get_dns_socket_handle();
        let socket = self.sockets.get_mut::<dns::Socket>(socket_handle);

        match socket.get_query_result(query_handle) {
            Err(dns::GetQueryResultError::Pending) => {
                if let Some(waker) = waker {
                    socket.register_query_waker(query_handle, waker);
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(crate::error::dns_result_error(e))),
            Ok(query_result) => Poll::Ready(Ok(query_result
                .iter()
                .map(IpAddress::from_smoltcp)
                .collect())),
        }
    }
}

/// Unknown controllers retain their cached link semantics. A failing controller
/// must not make a disconnected interface eligible for new sockets.
fn controller_link_state(controller: &dyn DirectCharacterDevice, fallback: bool) -> bool {
    use file_system::ControlCommand;
    use shared::AnyByLayout;
    let mut up = false;
    match controller.control_sync(
        crate::IS_LINK_UP::IDENTIFIER,
        AnyByLayout::from(&()),
        AnyByLayout::from_mutable(&mut up),
    ) {
        Ok(()) => up,
        Err(file_system::IoErrorKind::Unsupported) => fallback,
        Err(_) => false,
    }
}

#[cfg(test)]
mod dhcp_tests;

#[cfg(test)]
mod link_tests {
    use super::*;
    use file_system::{ControlCommandIdentifier, DirectBaseOperations, MountOperations};
    use shared::AnyByLayout;
    struct Controller(Option<file_system::Result<bool>>);
    impl MountOperations for Controller {}
    impl DirectCharacterDevice for Controller {}
    impl DirectBaseOperations for Controller {
        fn read(
            &self,
            _: &mut Context<'_>,
            _: &mut [u8],
            _: file_system::Size,
        ) -> Poll<file_system::Result<usize>> {
            Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
        }
        fn write(
            &self,
            _: &mut Context<'_>,
            _: &[u8],
            _: file_system::Size,
        ) -> Poll<file_system::Result<usize>> {
            Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
        }
        fn control(
            &self,
            _: &mut Context<'_>,
            _: ControlCommandIdentifier,
            _: &AnyByLayout,
            output: &mut AnyByLayout,
        ) -> Poll<file_system::Result<()>> {
            Poll::Ready(match self.0 {
                Some(Ok(value)) => {
                    *output.cast_mutable::<bool>().unwrap() = value;
                    Ok(())
                }
                Some(Err(error)) => Err(error),
                None => Err(file_system::IoErrorKind::Unsupported),
            })
        }
    }
    #[test]
    fn availability_requires_administrative_enable_and_effective_link() {
        let clock = Box::leak(Box::new(
            time::Manager::new(&testing_support::TimeDevice).unwrap(),
        ));
        let (mut packets, _) = crate::create_loopback_device();
        let mut stack = StackInner::new(
            "wifi",
            &mut packets,
            Controller(Some(Ok(false))),
            1,
            smoltcp::time::Instant::from_millis(0),
            clock,
        );
        assert!(stack.is_up); // the old cached default must not override disassociation
        assert!(!stack.is_available());
        stack.controller = Box::new(Controller(Some(Ok(true))));
        assert!(stack.is_available());
        stack.set_state(false);
        assert!(!stack.is_available());
    }

    #[test]
    fn effective_link_uses_controller_and_only_falls_back_when_unsupported() {
        assert!(!controller_link_state(&Controller(Some(Ok(false))), true));
        assert!(controller_link_state(&Controller(Some(Ok(true))), false));
        assert!(controller_link_state(&Controller(None), true));
        assert!(!controller_link_state(&Controller(None), false));
        assert!(!controller_link_state(
            &Controller(Some(Err(file_system::IoErrorKind::ResourceBusy))),
            true
        ));
        assert!(!controller_link_state(
            &Controller(Some(Err(file_system::IoErrorKind::InputOutput))),
            true
        ));
    }
}
