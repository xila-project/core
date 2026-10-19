use crate::{
    DnsQueryKind, Error, IpAddress, IpCidr, Ipv4, Ipv6, MacAddress, Port, Result, Route, WakeSignal,
};
use alloc::{boxed::Box, vec, vec::Vec};
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
use file_system::DirectCharacterDevice;
use shared::poll_pin_ready;
use smol_str::SmolStr;
use smoltcp::{
    config::{DNS_MAX_SERVER_COUNT, IFACE_MAX_ADDR_COUNT},
    iface::{self, SocketHandle, SocketSet},
    phy::{Device, Medium},
    socket::{AnySocket, Socket, dhcpv4, dns},
    wire::{self, DnsQueryType, EthernetAddress},
};
use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

pub struct StackInner {
    pub name: SmolStr,
    pub is_up: bool,
    pub enabled: bool,
    pub interface: smoltcp::iface::Interface,
    pub controller: Box<dyn DirectCharacterDevice>,
    pub sockets: smoltcp::iface::SocketSet<'static>,
    pub dhcp_socket: Option<smoltcp::iface::SocketHandle>,
    pub dns_socket: Option<SocketHandle>,
    pub dns_servers: Vec<smoltcp::wire::IpAddress>,
    pub maximum_transmission_unit: usize,
    pub maximum_burst_size: Option<usize>,
    pub next_local_port: Port,
    time_manager: &'static time::Manager<'static>,
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
            Medium::Ieee802154 => todo!(),
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
            dhcp_socket: None,
            dns_socket: None,
            dns_servers: Vec::with_capacity(DNS_MAX_SERVER_COUNT),
            maximum_transmission_unit: capabilities.max_transmission_unit,
            maximum_burst_size: capabilities.max_burst_size,
            next_local_port: Port::from_inner(next_local_port),
            time_manager,
        }
    }

    pub fn is_available(&self) -> bool {
        self.enabled && self.is_up
    }

    pub fn is_link_up(&self) -> bool {
        self.is_up
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

        self.interface
            .routes_mut()
            .update(|v| result = v.push(route.into_smoltcp()).map_err(|_| Error::NoFreeSlot));

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
            return Err(Error::NoFreeSlot);
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
            let dhcp_socket = dhcpv4::Socket::new();
            let handle = self.sockets.add(dhcp_socket);
            self.dhcp_socket = Some(handle);
        } else {
            self.dhcp_socket.take();
        }
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
            return Err(Error::NoFreeSlot);
        }

        let mut result = Ok(());

        self.interface.update_ip_addrs(|addrs| {
            result = addrs
                .push(cidr.into_smoltcp())
                .map_err(|_| Error::NoFreeSlot);
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

    pub fn poll(&mut self, device: &mut impl Device) -> Option<Duration> {
        let now = crate::get_smoltcp_time(self.time_manager);

        self.interface.poll(now, device, &mut self.sockets);

        let poll_at = self.interface.poll_at(now, &self.sockets);

        poll_at
            .map(|instant| instant - now)
            .map(|smoltcp_duration| Duration::from_micros(smoltcp_duration.total_micros()))
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

    pub fn start_dns_query(&mut self, name: &str, kind: DnsQueryKind) -> Result<dns::QueryHandle> {
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
            return Err(Error::UnsupportedProtocol);
        };

        let socket_handle = self.get_dns_socket_handle();
        let query_handle = self
            .sockets
            .get_mut::<dns::Socket>(socket_handle)
            .start_query(self.interface.context(), name, query_type)?;

        Ok(query_handle)
    }

    pub fn get_dns_query_result(
        &mut self,
        query_handle: dns::QueryHandle,
        waker: Option<&Waker>,
    ) -> Poll<Result<Vec<IpAddress>>> {
        let socket_handle = self.get_dns_socket_handle();
        let socket = self.sockets.get_mut::<dns::Socket>(socket_handle);

        match socket.get_query_result(query_handle) {
            Err(dns::GetQueryResultError::Pending) => {
                if let Some(waker) = waker {
                    socket.register_query_waker(query_handle, waker);
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err::<_, Error>(e.into())),
            Ok(query_result) => Poll::Ready(Ok(query_result
                .iter()
                .map(IpAddress::from_smoltcp)
                .collect())),
        }
    }
}
