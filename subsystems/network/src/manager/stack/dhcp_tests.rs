//! DHCP lease tests: the stack runs against a scripted DHCP server that lives
//! on a second smoltcp interface, joined to the stack by two packet queues.

extern crate std;

use super::*;
use core::cell::RefCell;
use smoltcp::{
    iface::{Config, Interface, SocketHandle},
    phy::{ChecksumCapabilities, DeviceCapabilities, RxToken, TxToken},
    socket::udp,
    storage::PacketMetadata,
    time::Instant,
    wire::{
        DhcpMessageType, DhcpPacket, DhcpRepr, HardwareAddress, IpAddress, IpCidr, IpEndpoint,
        Ipv4Address,
    },
};
use std::{collections::VecDeque, rc::Rc};

type Queue = Rc<RefCell<VecDeque<Vec<u8>>>>;

struct Wire {
    receive: Queue,
    transmit: Queue,
}

struct Receive(Vec<u8>);

impl RxToken for Receive {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

struct Transmit(Queue);

impl TxToken for Transmit {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, length: usize, f: F) -> R {
        let mut frame = vec![0; length];
        let result = f(&mut frame);
        self.0.borrow_mut().push_back(frame);
        result
    }
}

impl Device for Wire {
    type RxToken<'a> = Receive;
    type TxToken<'a> = Transmit;

    fn receive(&mut self, _: Instant) -> Option<(Receive, Transmit)> {
        let frame = self.receive.borrow_mut().pop_front()?;
        Some((Receive(frame), Transmit(self.transmit.clone())))
    }

    fn transmit(&mut self, _: Instant) -> Option<Transmit> {
        Some(Transmit(self.transmit.clone()))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = 1514;
        capabilities.checksum = ChecksumCapabilities::default();
        capabilities
    }
}

const SERVER: Ipv4Address = Ipv4Address::new(10, 0, 0, 1);
const LEASED: Ipv4Address = Ipv4Address::new(10, 0, 0, 42);
const DOMAIN_NAME_SERVER: Ipv4Address = Ipv4Address::new(10, 0, 0, 53);

struct Scenario {
    clock: &'static time::Manager<'static>,
    stack: StackInner,
    client_device: Wire,
    server: Interface,
    server_device: Wire,
    server_sockets: SocketSet<'static>,
    server_handle: SocketHandle,
    router: Option<Ipv4Address>,
}

impl Scenario {
    fn new() -> Self {
        let clock = Box::leak(Box::new(
            time::Manager::new(&testing_support::TimeDevice).unwrap(),
        ));
        let to_server: Queue = Default::default();
        let to_client: Queue = Default::default();
        let mut client_device = Wire {
            receive: to_client.clone(),
            transmit: to_server.clone(),
        };
        let mut server_device = Wire {
            receive: to_server,
            transmit: to_client,
        };

        let (_, controller) = crate::create_loopback_device();
        let now = crate::get_smoltcp_time(clock);
        let stack = StackInner::new("test", &mut client_device, controller, 1, now, clock);

        let mut server = Interface::new(
            Config::new(HardwareAddress::Ethernet(EthernetAddress([
                0x02, 0, 0, 0, 0, 0x02,
            ]))),
            &mut server_device,
            now,
        );
        server.update_ip_addrs(|addresses| {
            addresses
                .push(IpCidr::new(IpAddress::Ipv4(SERVER), 24))
                .unwrap();
        });
        let mut server_sockets = SocketSet::new(vec![]);
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(vec![PacketMetadata::EMPTY; 4], vec![0; 2048]),
            udp::PacketBuffer::new(vec![PacketMetadata::EMPTY; 4], vec![0; 2048]),
        );
        socket.bind(67).unwrap();
        let server_handle = server_sockets.add(socket);

        Self {
            clock,
            stack,
            client_device,
            server,
            server_device,
            server_sockets,
            server_handle,
            router: Some(SERVER),
        }
    }

    /// Answer every DISCOVER with an OFFER and every REQUEST with an ACK.
    fn serve(&mut self) {
        loop {
            let socket = self
                .server_sockets
                .get_mut::<udp::Socket>(self.server_handle);
            let Ok((payload, _)) = socket.recv() else {
                return;
            };
            let packet = DhcpPacket::new_checked(payload).unwrap();
            let request = DhcpRepr::parse(&packet).unwrap();
            let message_type = match request.message_type {
                DhcpMessageType::Discover => DhcpMessageType::Offer,
                DhcpMessageType::Request => DhcpMessageType::Ack,
                _ => continue,
            };
            let reply = DhcpRepr {
                message_type,
                transaction_id: request.transaction_id,
                secs: 0,
                client_hardware_address: request.client_hardware_address,
                client_ip: Ipv4Address::UNSPECIFIED,
                your_ip: LEASED,
                server_ip: SERVER,
                router: self.router,
                subnet_mask: Some(Ipv4Address::new(255, 255, 255, 0)),
                relay_agent_ip: Ipv4Address::UNSPECIFIED,
                broadcast: false,
                requested_ip: None,
                client_identifier: None,
                server_identifier: Some(SERVER),
                parameter_request_list: None,
                dns_servers: Some([DOMAIN_NAME_SERVER].iter().copied().collect()),
                max_size: None,
                lease_duration: Some(3600),
                renew_duration: None,
                rebind_duration: None,
                additional_options: &[],
            };
            let mut buffer = vec![0; reply.buffer_len()];
            reply
                .emit(&mut DhcpPacket::new_unchecked(&mut buffer[..]))
                .unwrap();
            let mut metadata = udp::UdpMetadata::from(IpEndpoint::new(
                IpAddress::Ipv4(Ipv4Address::BROADCAST),
                68,
            ));
            metadata.local_address = Some(IpAddress::Ipv4(SERVER));
            socket.send_slice(&buffer, metadata).unwrap();
        }
    }

    fn run(&mut self) {
        for _ in 0..20 {
            self.stack.poll(&mut self.client_device);
            let now = crate::get_smoltcp_time(self.clock);
            self.server
                .poll(now, &mut self.server_device, &mut self.server_sockets);
            self.serve();
            self.server
                .poll(now, &mut self.server_device, &mut self.server_sockets);
        }
    }

    fn addresses(&self) -> Vec<IpCidr> {
        self.stack.interface.ip_addrs().to_vec()
    }

    fn default_routes(&self) -> Vec<smoltcp::iface::Route> {
        let mut routes = Vec::new();
        self.stack
            .interface
            .routes()
            .get_default_ipv4_route()
            .into_iter()
            .for_each(|route| routes.push(route));
        routes
    }
}

fn leased_cidr() -> IpCidr {
    IpCidr::new(IpAddress::Ipv4(LEASED), 24)
}

fn static_cidr() -> IpCidr {
    IpCidr::new(IpAddress::Ipv4(Ipv4Address::new(192, 168, 50, 5)), 24)
}

#[test]
fn lease_applies_the_address_route_and_dns_servers() {
    let mut scenario = Scenario::new();
    scenario.stack.set_dhcp_state(true);
    scenario.run();

    assert_eq!(scenario.addresses(), vec![leased_cidr()]);
    let routes = scenario.default_routes();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].via_router, IpAddress::Ipv4(SERVER));
    assert_eq!(
        scenario.stack.get_dns_servers(),
        [IpAddress::Ipv4(DOMAIN_NAME_SERVER)]
    );
}

#[test]
fn disabling_dhcp_removes_the_socket_and_exactly_what_the_lease_added() {
    let mut scenario = Scenario::new();
    scenario
        .stack
        .add_ip_address(crate::IpCidr::from_smoltcp(&static_cidr()))
        .unwrap();
    scenario.stack.set_dhcp_state(true);
    scenario.run();
    assert_eq!(scenario.addresses(), vec![static_cidr(), leased_cidr()]);

    scenario.stack.set_dhcp_state(false);

    assert!(!scenario.stack.get_dhcp_state());
    assert_eq!(scenario.addresses(), vec![static_cidr()]);
    assert!(scenario.default_routes().is_empty());
    assert!(scenario.stack.get_dns_servers().is_empty());
    assert_eq!(scenario.stack.sockets.iter().count(), 0);
}

#[test]
fn a_deconfigured_lease_is_removed_and_a_new_one_is_applied_again() {
    let mut scenario = Scenario::new();
    scenario.stack.set_dhcp_state(true);
    scenario.run();
    assert_eq!(scenario.addresses(), vec![leased_cidr()]);

    let handle = scenario.stack.dhcp_socket.unwrap();
    scenario
        .stack
        .sockets
        .get_mut::<dhcpv4::Socket>(handle)
        .reset();
    scenario.stack.poll(&mut scenario.client_device);

    assert!(scenario.addresses().is_empty());
    assert!(scenario.default_routes().is_empty());
    assert!(scenario.stack.get_dns_servers().is_empty());
    assert!(scenario.stack.get_dhcp_state());

    scenario.run();
    assert_eq!(scenario.addresses(), vec![leased_cidr()]);
    assert_eq!(scenario.default_routes().len(), 1);
}

#[test]
fn static_configuration_is_kept_when_the_lease_comes_and_goes() {
    let mut scenario = Scenario::new();
    let static_router = Ipv4Address::new(192, 168, 50, 1);
    let static_server = IpAddress::Ipv4(Ipv4Address::new(192, 168, 50, 2));
    scenario
        .stack
        .add_ip_address(crate::IpCidr::from_smoltcp(&static_cidr()))
        .unwrap();
    scenario
        .stack
        .interface
        .routes_mut()
        .add_default_ipv4_route(static_router)
        .unwrap();
    scenario
        .stack
        .add_dns_server(crate::IpAddress::from_smoltcp(&static_server))
        .unwrap();

    scenario.stack.set_dhcp_state(true);
    scenario.run();

    // A static default route and a full DNS server list win over the lease.
    let routes = scenario.default_routes();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].via_router, IpAddress::Ipv4(static_router));
    assert_eq!(scenario.stack.get_dns_servers()[0], static_server);

    scenario.stack.set_dhcp_state(false);

    assert_eq!(scenario.addresses(), vec![static_cidr()]);
    let routes = scenario.default_routes();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].via_router, IpAddress::Ipv4(static_router));
    assert_eq!(scenario.stack.get_dns_servers(), [static_server]);
}

#[test]
fn a_lease_without_a_router_adds_no_route() {
    let mut scenario = Scenario::new();
    scenario.router = None;
    scenario.stack.set_dhcp_state(true);
    scenario.run();

    assert_eq!(scenario.addresses(), vec![leased_cidr()]);
    assert!(scenario.default_routes().is_empty());
}

#[test]
fn enabling_dhcp_twice_keeps_one_socket() {
    let mut scenario = Scenario::new();
    scenario.stack.set_dhcp_state(true);
    let handle = scenario.stack.dhcp_socket;
    scenario.stack.set_dhcp_state(true);

    assert_eq!(scenario.stack.dhcp_socket, handle);
    assert_eq!(scenario.stack.sockets.iter().count(), 1);

    scenario.stack.set_dhcp_state(false);
    scenario.stack.set_dhcp_state(false);
    assert!(!scenario.stack.get_dhcp_state());
    assert_eq!(scenario.stack.sockets.iter().count(), 0);
}
