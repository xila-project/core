//! Full HTTPS device test using a local Python/OpenSSL TLS server. The relay
//! connects Xila's isolated smoltcp loopback to the host's loopback socket;
//! TLS records pass through unchanged, including the real handshake.
extern crate abi_definitions;
extern crate alloc;
use network::TcpSocket;
drivers_std::memory::instantiate_global_allocator!();
use alloc::{format, vec::Vec};
use core::{future::poll_fn, time::Duration};
use file_system::{BaseOperations, Context, Error};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
};

struct Server {
    child: Child,
    directory: std::path::PathBuf,
    port: u16,
}
impl Server {
    fn start() -> Self {
        let directory = std::env::temp_dir().join(format!("xila-https-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let key = directory.join("key.pem");
        let cert = directory.join("cert.pem");
        assert!(
            Command::new("openssl")
                .args([
                    "req",
                    "-x509",
                    "-newkey",
                    "ec",
                    "-pkeyopt",
                    "ec_paramgen_curve:P-256",
                    "-nodes",
                    "-days",
                    "1",
                    "-subj",
                    "/CN=localhost",
                    "-keyout"
                ])
                .arg(&key)
                .arg("-out")
                .arg(&cert)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("openssl required")
                .success()
        );
        let mut child = Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/https_server.py"
            ))
            .arg(cert)
            .arg(key)
            .stdout(Stdio::piped())
            .spawn()
            .expect("python3 required");
        let mut line = std::string::String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = line.trim().parse().unwrap();
        Self {
            child,
            directory,
            port,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

async fn relay(mut socket: TcpSocket, host_port: u16) {
    socket
        .accept(Some([127, 0, 0, 1]), network::Port::from_inner(51020))
        .await
        .unwrap();
    let mut host = std::net::TcpStream::connect(("127.0.0.1", host_port)).unwrap();
    host.set_nonblocking(true).unwrap();
    let mut buffer = [0; 4096];
    let deadline = std::time::Instant::now() + Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        match host.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let mut sent = 0;
                while sent < n {
                    match socket.write(&buffer[sent..n]).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => sent += n,
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break,
        }
        match embassy_futures::select::select(
            socket.read(&mut buffer),
            task::sleep(Duration::from_millis(1)),
        )
        .await
        {
            embassy_futures::select::Either::First(Ok(0) | Err(_)) => break,
            embassy_futures::select::Either::First(Ok(n)) => {
                let mut sent = 0;
                while sent < n {
                    match host.write(&buffer[sent..n]) {
                        Ok(0) => return,
                        Ok(n) => sent += n,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            task::sleep(Duration::from_millis(1)).await
                        }
                        Err(_) => return,
                    }
                }
            }
            embassy_futures::select::Either::Second(()) => {}
        }
    }
    socket.close().await;
}

#[task::test]
#[ignore = "requires python3 and openssl; local TLS 1.3 end-to-end test"]
async fn https_device_with_real_tls_server() {
    let standard = testing::initialize(false, false).await;
    let context = standard.context;
    let tasks = context.task_manager;
    static ENTROPY: drivers_shared::devices::RandomDevice = drivers_shared::devices::RandomDevice;
    let manager = Box::leak(Box::new(network::Manager::new(
        tasks,
        context.virtual_file_system,
        context.time_manager,
        &ENTROPY,
    )));
    let (device, controller) = network::create_loopback_device();
    let spawner = drivers_std::executor::new_thread_executor(tasks).await;
    let task = tasks.get_current_task_identifier().await;
    manager
        .mount_interface(task, "tls-loopback", device, controller, Some(spawner))
        .await
        .unwrap();
    let mut interface = virtual_file_system::File::open(
        context.virtual_file_system,
        task,
        "/devices/network/tls-loopback",
        file_system::AccessFlags::Write.into(),
    )
    .await
    .unwrap();
    interface
        .control(
            network::ADD_IP_ADDRESS,
            &network::IpCidr::new_ipv4([127, 0, 0, 1], 8),
        )
        .await
        .unwrap();
    interface.close(context.virtual_file_system).await.unwrap();
    let server = Server::start();
    static RANDOM: drivers_shared::devices::RandomDevice = drivers_shared::devices::RandomDevice;
    let device = drivers_shared::devices::HttpsClientDevice::new(&RANDOM, manager, tasks);
    for (path, length, truncated) in [
        ("/length", 11, false),
        ("/chunked", 11, false),
        ("/close", 11, false),
        ("/interim", 11, false),
        ("/large", 262144, false),
        ("/truncated", 5, true),
        ("/truncated-chunk", 5, true),
    ] {
        let listener = manager.new_tcp_socket(4096, 4096, None).await.unwrap();
        listener.with_mutable(|s| s.listen(51020).unwrap()).await;
        let port = server.port;
        let (relay_task, _) = tasks
            .spawn(
                task::Manager::ROOT_TASK_IDENTIFIER,
                "TLS test relay",
                None,
                move |_| async move {
                    relay(listener, port).await;
                },
            )
            .await
            .unwrap();
        let mut context = Context::new_empty();
        device.open(&mut context).unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:51020\r\nConnection: close\r\n\r\n");
        let started = std::time::Instant::now();
        let operation = async {
            poll_fn(|cx| device.write(&mut context, cx, request.as_bytes(), 0))
                .await
                .unwrap();
            let mut head = Vec::new();
            let mut buffer = [0; 4096];
            while shared::get_body(&head).is_none() {
                let n = poll_fn(|cx| device.read(&mut context, cx, &mut buffer, 0))
                    .await
                    .expect(path);
                assert!(n > 0, "missing head: {path}");
                head.extend_from_slice(&buffer[..n]);
            }
            assert_eq!(
                shared::HttpResponseParser::from_buffer(&head).get_status_code(),
                Some(200)
            );
            let mut total = 0;
            let mut body = Vec::new();
            loop {
                match poll_fn(|cx| device.read(&mut context, cx, &mut buffer, 0)).await {
                    Ok(0) => {
                        assert!(!truncated, "silent truncation: {path}");
                        break;
                    }
                    Ok(n) => {
                        total += n;
                        if path != "/large" {
                            body.extend_from_slice(&buffer[..n]);
                        } else {
                            assert!(buffer[..n].iter().all(|b| *b == b'x'));
                        }
                    }
                    Err(error) => {
                        assert!(truncated, "unexpected {error:?}: {path}");
                        assert_eq!(error, Error::InputOutput);
                        break;
                    }
                }
            }
            assert_eq!(total, length, "body length: {path}");
            if !truncated && path != "/large" {
                assert_eq!(&body, b"hello world");
            }
        };
        assert!(
            matches!(
                embassy_futures::select::select(operation, task::sleep(Duration::from_secs(10)))
                    .await,
                embassy_futures::select::Either::First(())
            ),
            "HTTPS timed out: {path}"
        );
        device.close(&mut context).unwrap();
        relay_task.join().await;
        std::println!(
            "HTTPS {path}: {length} body bytes, truncation={truncated}, {:?}",
            started.elapsed()
        );
    }
}
