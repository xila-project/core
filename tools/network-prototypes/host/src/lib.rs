//! Experimental native host-socket boundary for issue #413. Not wired into
//! network::Manager: production networking and embedded builds are unchanged.
mod resolver;
use async_io::Async;
use network::{Error, IpAddress, Port, Result};
pub use resolver::Resolver;
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Limits {
    maximum: usize,
    active: AtomicUsize,
}
struct Lease(Arc<Limits>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One shared native reactor (async-io), with a strict bound on owned sockets.
/// Socket buffers remain host-kernel-owned; this bound is not a RAM budget.
#[derive(Clone)]
pub struct HostProvider {
    limits: Arc<Limits>,
}
impl HostProvider {
    pub fn new(maximum: usize) -> Result<Self> {
        if maximum == 0 {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            limits: Arc::new(Limits {
                maximum,
                active: AtomicUsize::new(0),
            }),
        })
    }
    pub fn active_sockets(&self) -> usize {
        self.limits.active.load(Ordering::Acquire)
    }
    fn reserve(&self) -> Result<Lease> {
        reserve(&self.limits)
    }
    pub async fn connect(&self, address: IpAddress, port: Port) -> Result<HostTcpSocket> {
        let lease = self.reserve()?;
        let socket = Async::<TcpStream>::connect(endpoint(address, port))
            .await
            .map_err(map_io)?;
        Ok(HostTcpSocket {
            socket,
            _lease: lease,
        })
    }
    pub fn bind_udp(&self, address: IpAddress, port: Port) -> Result<HostUdpSocket> {
        let lease = self.reserve()?;
        let socket = UdpSocket::bind(endpoint(address, port)).map_err(map_io)?;
        Ok(HostUdpSocket {
            socket: Async::new(socket).map_err(map_io)?,
            _lease: lease,
        })
    }
}

pub struct HostTcpSocket {
    socket: Async<TcpStream>,
    _lease: Lease,
}
impl HostTcpSocket {
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.socket
            .read_with(|mut stream| stream.read(buffer))
            .await
            .map_err(map_io)
    }
    pub async fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        self.socket
            .write_with(|mut stream| stream.write(buffer))
            .await
            .map_err(map_io)
    }
    pub async fn write_all(&mut self, mut buffer: &[u8]) -> Result<()> {
        while !buffer.is_empty() {
            let n = self.write(buffer).await?;
            if n == 0 {
                return Err(Error::WriteZero);
            }
            buffer = &buffer[n..];
        }
        Ok(())
    }
    pub async fn flush(&mut self) -> Result<()> {
        // Host writes enqueue directly in the OS; this does not promise ACK.
        self.socket
            .write_with(|mut stream| stream.flush())
            .await
            .map_err(map_io)
    }
    pub fn shutdown_write(&self) -> Result<()> {
        self.socket
            .get_ref()
            .shutdown(std::net::Shutdown::Write)
            .map_err(map_io)
    }
}

pub struct HostUdpSocket {
    socket: Async<UdpSocket>,
    _lease: Lease,
}
impl HostUdpSocket {
    pub async fn write_to(&self, buffer: &[u8], address: IpAddress, port: Port) -> Result<()> {
        let destination = endpoint(address, port);
        let n = self
            .socket
            .write_with(|s| s.send_to(buffer, destination))
            .await
            .map_err(map_io)?;
        if n == buffer.len() {
            Ok(())
        } else {
            Err(Error::WriteZero)
        }
    }
    pub async fn read_from(&self, buffer: &mut [u8]) -> Result<(usize, IpAddress, Port)> {
        let (n, peer) = self
            .socket
            .read_with(|s| s.recv_from(buffer))
            .await
            .map_err(map_io)?;
        let address = match peer.ip() {
            IpAddr::V4(a) => IpAddress::from(a.octets()),
            IpAddr::V6(a) => IpAddress::from(a.octets()),
        };
        Ok((n, address, Port::from_inner(peer.port())))
    }
}
fn endpoint(address: IpAddress, port: Port) -> SocketAddr {
    let ip = match address {
        IpAddress::IPv4(a) => IpAddr::from(a.into_inner()),
        IpAddress::IPv6(a) => IpAddr::from(a.into_inner()),
    };
    SocketAddr::new(ip, port.into_inner())
}
fn map_io(error: io::Error) -> Error {
    match error.kind() {
        io::ErrorKind::NotFound => Error::NotFound,
        io::ErrorKind::PermissionDenied => Error::PermissionDenied,
        io::ErrorKind::ConnectionRefused => Error::ConnectionRefused,
        io::ErrorKind::ConnectionReset => Error::ConnectionReset,
        io::ErrorKind::ConnectionAborted => Error::ConnectionAborted,
        io::ErrorKind::NotConnected => Error::NotConnected,
        io::ErrorKind::AddrInUse => Error::AddressInUse,
        io::ErrorKind::AddrNotAvailable => Error::AddressNotAvailable,
        io::ErrorKind::WouldBlock => Error::WouldBlock,
        io::ErrorKind::TimedOut => Error::TimedOut,
        io::ErrorKind::WriteZero => Error::WriteZero,
        io::ErrorKind::InvalidInput => Error::InvalidInput,
        io::ErrorKind::InvalidData => Error::InvalidData,
        io::ErrorKind::Interrupted => Error::Interrupted,
        io::ErrorKind::UnexpectedEof => Error::UnexpectedEndOfFile,
        _ => Error::Other,
    }
}

fn reserve(limits: &Arc<Limits>) -> Result<Lease> {
    let mut current = limits.active.load(Ordering::Acquire);
    loop {
        if current >= limits.maximum {
            return Err(Error::NoFreeSlot);
        }
        match limits.active.compare_exchange_weak(
            current,
            current + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return Ok(Lease(limits.clone())),
            Err(actual) => current = actual,
        }
    }
}
