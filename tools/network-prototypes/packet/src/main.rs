use smoltcp::{
    iface::{Config, Interface, SocketSet},
    socket::{dns, tcp, udp},
    time::Instant,
    wire::{DnsQueryType, EthernetAddress, IpAddress, IpCidr, Ipv4Address},
};
use std::{
    error::Error,
    io::{self, Read, Write},
    net::{TcpListener, UdpSocket},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant as Clock},
};
use xila_packet_network_prototype::PacketDevice;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const GATEWAY: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
const DNS: Ipv4Address = Ipv4Address::new(10, 0, 2, 3);
const EXPECTED: Ipv4Address = Ipv4Address::new(203, 0, 113, 9);
const MESSAGE: &[u8] = b"xila packet prototype";

struct Helper {
    child: Child,
    directory: PathBuf,
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
struct Servers {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Drop for Servers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn local_servers() -> Result<(Servers, u16, u16)> {
    let tcp = TcpListener::bind(("127.0.0.1", 0))?;
    let udp = UdpSocket::bind(("127.0.0.1", 0))?;
    // No system resolver changes: an isolated local fixture address. A host that
    // reserves low ports will report PermissionDenied here, before helper launch.
    let dns = UdpSocket::bind(("127.0.0.2", 53))
        .map_err(|e| io::Error::new(e.kind(), format!("local DNS fixture 127.0.0.2:53: {e}")))?;
    let tcp_port = tcp.local_addr()?.port();
    let udp_port = udp.local_addr()?.port();
    tcp.set_nonblocking(true)?;
    udp.set_nonblocking(true)?;
    dns.set_nonblocking(true)?;
    let mut servers = Servers {
        stop: Arc::new(AtomicBool::new(false)),
        threads: vec![],
    };
    let stop = servers.stop.clone();
    servers.threads.push(thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if let Ok((mut stream, _)) = tcp.accept() {
                let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                let mut request = [0; MESSAGE.len()];
                if stream.read_exact(&mut request).is_ok() {
                    let _ = stream.write_all(&request);
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    }));
    for (socket, is_dns) in [(udp, false), (dns, true)] {
        let stop = servers.stop.clone();
        servers.threads.push(thread::spawn(move || {
            let mut buffer = [0u8; 512];
            while !stop.load(Ordering::Relaxed) {
                if let Ok((len, peer)) = socket.recv_from(&mut buffer) {
                    if is_dns && len >= 12 && len + 16 <= buffer.len() {
                        // Preserve transaction ID/question; one compressed A answer.
                        buffer[2..4].copy_from_slice(&[0x81, 0x80]);
                        buffer[6..12].copy_from_slice(&[0, 1, 0, 0, 0, 0]);
                        buffer[len..len + 16].copy_from_slice(&[
                            0xc0, 0x0c, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 203, 0, 113, 9,
                        ]);
                        let _ = socket.send_to(&buffer[..len + 16], peer);
                    } else if !is_dns {
                        let _ = socket.send_to(&buffer[..len], peer);
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
        }));
    }
    Ok((servers, tcp_port, udp_port))
}

fn main() -> Result<()> {
    let passt = std::env::args_os()
        .nth(1)
        .ok_or("usage: xila-packet-network-prototype /path/to/passt")?;
    let (_servers, tcp_port, udp_port) = local_servers()?;
    let directory = std::env::temp_dir().join(format!("xila-packet-{}", std::process::id()));
    std::fs::create_dir(&directory)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let socket_path = directory.join("stream.socket");
    let child = Command::new(passt)
        .args(["--foreground", "--quiet", "--one-off", "--socket"])
        .arg(&socket_path)
        .args([
            "--address",
            "10.0.2.15",
            "--netmask",
            "24",
            "--gateway",
            "10.0.2.2",
            "--map-host-loopback",
            "10.0.2.2",
            "--dns-forward",
            "10.0.2.3",
            "--dns-host",
            "127.0.0.2",
            "--ipv4-only",
            "--mtu",
            "1500",
            "--tcp-ports",
            "none",
            "--udp-ports",
            "none",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn();
    let child = match child {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_dir_all(directory);
            return Err(e.into());
        }
    };
    let mut helper = Helper { child, directory };
    let started = Clock::now();
    let stream = loop {
        if let Some(status) = helper.child.try_wait()? {
            return Err(format!("passt exited during startup: {status}").into());
        }
        match UnixStream::connect(&socket_path) {
            Ok(stream) => break stream,
            Err(e)
                if started.elapsed() < Duration::from_secs(3)
                    && matches!(
                        e.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    };
    stream.set_nonblocking(true)?;
    let mut device = PacketDevice::new(stream);
    let mut config = Config::new(EthernetAddress([0x02, 0, 0, 0, 0, 1]).into());
    config.random_seed = 0x413;
    let mut iface = Interface::new(config, &mut device, Instant::from_millis(0));
    iface.update_ip_addrs(|addrs| {
        addrs
            .push(IpCidr::new(Ipv4Address::new(10, 0, 2, 15).into(), 24))
            .unwrap();
    });
    iface.routes_mut().add_default_ipv4_route(GATEWAY).unwrap();
    let mut sockets = SocketSet::new(vec![]);
    let tcp = sockets.add(tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 4096]),
        tcp::SocketBuffer::new(vec![0; 4096]),
    ));
    let mut udp_socket = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]),
    );
    udp_socket.bind(49153).unwrap();
    udp_socket
        .send_slice(MESSAGE, (IpAddress::Ipv4(GATEWAY), udp_port))
        .unwrap();
    let udp = sockets.add(udp_socket);
    let dns = sockets.add(dns::Socket::new(&[DNS.into()], vec![None]));
    let query = sockets
        .get_mut::<dns::Socket>(dns)
        .start_query(iface.context(), "fixture.test", DnsQueryType::A)
        .unwrap();
    sockets
        .get_mut::<tcp::Socket>(tcp)
        .connect(iface.context(), (GATEWAY, tcp_port), 49152)
        .unwrap();
    let mut sent = 0;
    let mut tcp_response = Vec::with_capacity(MESSAGE.len());
    let (mut udp_ok, mut dns_ok) = (false, false);
    let mut poll_count = 0u64;
    let workload_started = Clock::now();
    while workload_started.elapsed() < Duration::from_secs(5) {
        iface.poll(
            Instant::from_millis(workload_started.elapsed().as_millis() as i64),
            &mut device,
            &mut sockets,
        );
        if let Some(error) = device.error() {
            return Err(io::Error::from(error).into());
        }
        poll_count += 1;
        let socket = sockets.get_mut::<tcp::Socket>(tcp);
        if sent < MESSAGE.len() && socket.can_send() {
            sent += socket.send_slice(&MESSAGE[sent..]).unwrap();
        }
        if socket.can_recv() {
            let mut b = [0; 128];
            let n = socket.recv_slice(&mut b).unwrap();
            tcp_response.extend_from_slice(&b[..n]);
            if tcp_response.len() > MESSAGE.len() {
                return Err("unexpected TCP response length".into());
            }
        }
        if !udp_ok {
            let socket = sockets.get_mut::<udp::Socket>(udp);
            if socket.can_recv() {
                let (bytes, endpoint) = socket.recv().unwrap();
                if bytes != MESSAGE || endpoint.endpoint != (GATEWAY, udp_port).into() {
                    return Err("UDP echo mismatch".into());
                }
                udp_ok = true;
            }
        }
        if !dns_ok {
            match sockets.get_mut::<dns::Socket>(dns).get_query_result(query) {
                Ok(answer) if answer.as_slice() == [IpAddress::Ipv4(EXPECTED)] => dns_ok = true,
                Ok(_) => return Err("DNS answer mismatch".into()),
                Err(dns::GetQueryResultError::Pending) => {}
                Err(e) => return Err(format!("DNS query failed: {e:?}").into()),
            }
        }
        if tcp_response == MESSAGE && udp_ok && dns_ok {
            println!(
                "PASS local TCP echo ({} bytes), UDP echo ({} bytes), DNS fixture.test = {}",
                MESSAGE.len(),
                MESSAGE.len(),
                EXPECTED
            );
            println!(
                "elapsed_ms={} polls={} packet_device_storage_bytes={} queue_capacity=8 frame_max=1514",
                workload_started.elapsed().as_millis(),
                poll_count,
                device.storage_bytes()
            );
            if let Ok(status) =
                std::fs::read_to_string(format!("/proc/{}/status", helper.child.id()))
            {
                println!("helper_pid={}", helper.child.id());
                for line in status.lines().filter(|line| {
                    line.starts_with("VmRSS:")
                        || line.starts_with("VmSize:")
                        || line.starts_with("Threads:")
                }) {
                    println!("helper_{line}");
                }
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(format!(
        "5s timeout: tcp_bytes={} udp={udp_ok} dns={dns_ok}",
        tcp_response.len()
    )
    .into())
}
