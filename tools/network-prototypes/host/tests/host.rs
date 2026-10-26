use futures_lite::future::{block_on, race};
fn run<T>(future: impl core::future::Future<Output = T>) -> T {
    block_on(race(future, async {
        async_io::Timer::after(std::time::Duration::from_secs(5)).await;
        panic!("host prototype operation exceeded five-second test deadline")
    }))
}
use network::{Error, IpAddress, Port};
use std::{
    io::{Read, Write},
    net::{TcpListener, UdpSocket},
    thread,
};
use xila_host_network_prototype::HostProvider;

#[test]
fn tcp_round_trip_half_close_and_quota_cleanup() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = [0; 4];
        stream.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"ping");
        stream.write_all(b"pong").unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
    });
    run(async {
        let provider = HostProvider::new(1).unwrap();
        let mut socket = provider
            .connect(IpAddress::new_ipv4([127, 0, 0, 1]), Port::from_inner(port))
            .await
            .unwrap();
        assert_eq!(provider.active_sockets(), 1);
        assert!(matches!(
            provider.bind_udp(IpAddress::new_ipv4([127, 0, 0, 1]), Port::from_inner(0)),
            Err(Error::NoFreeSlot)
        ));
        socket.write_all(b"ping").await.unwrap();
        let mut bytes = [0; 4];
        let mut n = 0;
        while n < 4 {
            let got = socket.read(&mut bytes[n..]).await.unwrap();
            assert!(got > 0, "peer closed before full reply");
            n += got;
        }
        assert_eq!(&bytes, b"pong");
        assert_eq!(socket.read(&mut bytes).await.unwrap(), 0);
        drop(socket);
        assert_eq!(provider.active_sockets(), 0);
    });
    server.join().unwrap();
}

#[test]
fn udp_round_trip() {
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let port = server.local_addr().unwrap().port();
    let worker = thread::spawn(move || {
        let mut bytes = [0; 16];
        let (n, peer) = server.recv_from(&mut bytes).unwrap();
        server.send_to(&bytes[..n], peer).unwrap();
    });
    run(async {
        let provider = HostProvider::new(2).unwrap();
        let socket = provider
            .bind_udp(IpAddress::new_ipv4([127, 0, 0, 1]), Port::from_inner(0))
            .unwrap();
        socket
            .write_to(
                b"datagram",
                IpAddress::new_ipv4([127, 0, 0, 1]),
                Port::from_inner(port),
            )
            .await
            .unwrap();
        let mut bytes = [0; 16];
        let (n, _, source) = socket.read_from(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..n], b"datagram");
        assert_eq!(source.into_inner(), port);
        drop(socket);
        assert_eq!(provider.active_sockets(), 0);
    });
    worker.join().unwrap();
}

#[test]
fn refused_connect_releases_quota() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    run(async {
        let provider = HostProvider::new(1).unwrap();
        assert!(matches!(
            provider
                .connect(IpAddress::new_ipv4([127, 0, 0, 1]), Port::from_inner(port))
                .await,
            Err(Error::ConnectionRefused)
        ));
        assert_eq!(provider.active_sockets(), 0);
    });
}

#[test]
fn cancelled_pending_read_releases_socket_quota() {
    use core::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
    });
    run(async {
        let provider = HostProvider::new(1).unwrap();
        let mut socket = provider
            .connect(IpAddress::new_ipv4([127, 0, 0, 1]), Port::from_inner(port))
            .await
            .unwrap();
        let mut bytes = [0; 4];
        {
            let mut read = pin!(socket.read(&mut bytes));
            assert_eq!(
                read.as_mut().poll(&mut Context::from_waker(Waker::noop())),
                Poll::Pending
            );
        }
        drop(socket);
        assert_eq!(provider.active_sockets(), 0);
    });
    release.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn bounded_host_resolver_returns_localhost_without_blocking_poll() {
    use xila_host_network_prototype::Resolver;
    let resolver = Resolver::new(2).unwrap();
    let addresses = run(resolver.lookup("localhost")).unwrap();
    assert!(!addresses.is_empty());
    assert!(
        addresses
            .iter()
            .any(|ip| matches!(ip,IpAddress::IPv4(v) if v.into_inner() == [127,0,0,1]))
    );
    assert!(run(resolver.lookup("bad\0host")).is_err());
}

#[test]
fn ipv6_tcp_and_udp_are_tested_independently() {
    let listener = TcpListener::bind("[::1]:0").unwrap();
    let tcp = listener.local_addr().unwrap().port();
    let tcp_worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.write_all(b"ipv6").unwrap();
    });
    let server = UdpSocket::bind("[::1]:0").unwrap();
    server
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let udp = server.local_addr().unwrap().port();
    let udp_worker = thread::spawn(move || {
        let mut bytes = [0; 4];
        let (n, peer) = server.recv_from(&mut bytes).unwrap();
        server.send_to(&bytes[..n], peer).unwrap();
    });
    run(async {
        let provider = HostProvider::new(2).unwrap();
        let loopback = IpAddress::new_ipv6([0, 0, 0, 0, 0, 0, 0, 1]);
        let mut stream = provider
            .connect(loopback.clone(), Port::from_inner(tcp))
            .await
            .unwrap();
        let mut bytes = [0; 4];
        let mut n = 0;
        while n < 4 {
            let got = stream.read(&mut bytes[n..]).await.unwrap();
            assert!(got > 0);
            n += got;
        }
        assert_eq!(&bytes, b"ipv6");
        let socket = provider
            .bind_udp(loopback.clone(), Port::from_inner(0))
            .unwrap();
        socket
            .write_to(b"ipv6", loopback.clone(), Port::from_inner(udp))
            .await
            .unwrap();
        let (n, source, _) = socket.read_from(&mut bytes).await.unwrap();
        assert_eq!(source, loopback);
        assert_eq!(&bytes[..n], b"ipv6");
    });
    tcp_worker.join().unwrap();
    udp_worker.join().unwrap();
}
