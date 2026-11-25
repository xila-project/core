//! SNTP client for Xila: sets the system UTC clock from the network.
//!
//! The protocol is not reimplemented here. The [`sntpc`] crate (RFC 4330 and
//! RFC 5905, SNTP version 4 over UDP port 123) builds and validates the
//! packets, reconstructs the NTP era and computes the offset from all four
//! timestamps. This module only adapts it to Xila: a [`sntpc::NtpUdpSocket`]
//! over the network subsystem's UDP socket, a [`sntpc::NtpTimestampGenerator`]
//! over the time manager, and the application of the result through
//! `time::Manager::set_realtime` with `ClockSource::Network`.
//!
//! The client lives in the network subsystem because it is a network service
//! owned by that runtime. It resolves server names with
//! [`Manager::resolve`](crate::Manager::resolve) and opens its socket with
//! [`Manager::new_udp_socket`](crate::Manager::new_udp_socket), and it needs
//! the task spawner, the virtual file system (for the configuration file) and
//! the time manager, which are already its dependencies. The UDP socket is
//! created once and reused (it buffers 4 datagrams); DNS resolution returns a
//! heap `Vec` per name, cached for one hour.
//!
//! # Safety bounds
//!
//! Answers before the platform's minimum timestamp (the build date on devices
//! without a battery clock) are rejected. After the first network
//! synchronization, a clock step larger than 1000 s is applied only when a
//! second answer confirms it: the signed offset of the two answers (candidate
//! minus the current clock) must agree within 5 s. A confirming answer from
//! another server or resolved address is preferred; with a single server, a
//! later exchange of the same server confirms, so a single-server setup only
//! resists a blind off-path attacker. Unauthenticated SNTP cannot stop an
//! on-path attacker, and `sntpc` 0.11 does not randomize the low bits of the
//! transmit timestamp as RFC 5905 section 8 asks; each exchange sends from a
//! fresh source port instead (the stack's port counter is sequential).
//!
//! # Configuration and the NTP Pool
//!
//! Servers come from `/configuration/system/sntp.json` (default
//! `pool.ntp.org`, resynchronized every hour). The default server name is a
//! placeholder only: the NTP Pool asks vendors not to ship the standard
//! `pool.ntp.org` names as a default configuration ("You must absolutely not
//! use the default pool.ntp.org zone names as the default configuration in your
//! application or appliance."), and to request a vendor zone such as
//! `xila.pool.ntp.org` instead. See <https://www.ntppool.org/vendors.html>.
//!
//! [`Service::start`] spawns the synchronization task: it waits until an
//! interface has an address, queries the configured servers at random times,
//! then resynchronizes periodically with a bounded exponential backoff on
//! failure. Kiss-of-death answers ban or slow down a server per RFC 5905
//! section 7.4.

mod client;
mod configuration;
mod service;

pub use service::Service;

#[cfg(test)]
pub(crate) mod support {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::time::Duration;

    use smoltcp::phy::PacketMeta;
    use synchronization::{
        Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal,
    };

    use super::client::SERVER_PORT;
    use crate::tests::{initialize, task_manager};
    use crate::{Port, UdpMetadata};

    pub(crate) static TEST_MUTEX: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

    /// Seconds between the NTP epoch (1900-01-01) and the Unix epoch.
    const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

    pub(crate) fn host_time_manager() -> &'static time::Manager<'static> {
        Box::leak(Box::new(
            time::Manager::new(&testing_support::TimeDevice).unwrap(),
        ))
    }

    /// The NTP wire timestamp (seconds and fraction) of a Unix time in
    /// nanoseconds.
    pub(crate) fn ntp_wire(unix_nanos: i128) -> u64 {
        let seconds = (unix_nanos / 1_000_000_000 + NTP_UNIX_OFFSET as i128) as u64;
        let fraction = (((unix_nanos % 1_000_000_000) as u64) << 32) / 1_000_000_000;
        (seconds << 32) | fraction
    }

    /// Spawn a scripted server on 127.0.0.1:123 that reads `requests`
    /// requests and sends back the answers `answer` builds from each
    /// request's transmit timestamp (bytes 40 to 48), in order.
    pub(crate) async fn spawn_scripted_server(
        answer: impl Fn([u8; 48]) -> Vec<[u8; 48]> + Send + 'static,
        requests: usize,
    ) -> (
        Arc<Signal<CriticalSectionRawMutex, ()>>,
        task::JoinHandle<()>,
    ) {
        let network_manager = initialize().await;
        let mut server_socket = network_manager
            .new_udp_socket(64, 64, 4, 4, None)
            .await
            .expect("server socket");
        server_socket
            .bind(Port::from_inner(SERVER_PORT))
            .await
            .expect("server bind");

        let ready = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let ready_clone = ready.clone();
        let (server_task, _) = task_manager()
            .spawn_send(
                task::Manager::ROOT_TASK_IDENTIFIER,
                "SNTP scripted server",
                None,
                move |_| async move {
                    ready_clone.signal(());
                    for _ in 0..requests {
                        let mut buffer = [0u8; 64];
                        let (size, remote) = server_socket
                            .read_from(&mut buffer)
                            .await
                            .expect("request received");
                        assert_eq!(size, 48, "request is 48 bytes");
                        assert_eq!(buffer[0], 0x23, "leap 0, version 4, mode 3");
                        let mut request = [0u8; 48];
                        request.copy_from_slice(&buffer[..48]);
                        let metadata = UdpMetadata::new(
                            remote.remote_address,
                            remote.remote_port,
                            None,
                            PacketMeta::default(),
                        );
                        for response in answer(request) {
                            server_socket
                                .write_to(&response, &metadata)
                                .await
                                .expect("answer sent");
                        }
                    }
                    // `close` drops a queued packet, and `flush` only checks
                    // capacity: give the runner time to transmit the answers.
                    task::sleep(Duration::from_millis(100)).await;
                    server_socket.close().await;
                },
            )
            .await
            .unwrap();
        (ready, server_task)
    }

    /// A valid server answer: mode 4, version 4, stratum 2, echo of the
    /// request transmit timestamp, and the given server transmit time.
    pub(crate) fn valid_answer(server_transmit_unix_nanos: i128) -> impl Fn([u8; 48]) -> [u8; 48] {
        move |request| {
            let mut response = [0u8; 48];
            response[0] = 0x24; // leap 0, version 4, mode 4
            response[1] = 2; // stratum
            response[3] = 0xEC; // precision -20
            response[8..12].copy_from_slice(&0x0000_1000u32.to_be_bytes()); // root dispersion 1/16 s
            response[24..32].copy_from_slice(&request[40..48]); // originate echo
            let transmit = ntp_wire(server_transmit_unix_nanos);
            response[32..40].copy_from_slice(&transmit.to_be_bytes()); // receive
            response[40..48].copy_from_slice(&transmit.to_be_bytes()); // transmit
            response[16..24].copy_from_slice(&transmit.to_be_bytes()); // reference
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use super::client::{Client, Failure};
    use super::support::*;
    use crate::tests::initialize;
    use crate::{DnsQueryKind, IpAddress};

    /// Resolve `server` and run one exchange against its addresses.
    async fn exchange(
        network: &crate::Manager,
        client: &Client<'_>,
        server: &str,
        socket: &super::client::SntpUdpSocket,
    ) -> core::result::Result<super::client::Exchange, Failure> {
        let addresses = network
            .resolve(server, DnsQueryKind::A | DnsQueryKind::Aaaa, true, None)
            .await
            .map_err(|error| Failure::Io(error.kind()))?;
        client.exchange_addresses(&addresses, socket).await
    }

    /// A scripted server on the loopback answers one request through `sntpc`
    /// with a transmit time one hour ahead of the host clock, and the client
    /// sets UTC from it.
    #[task::test]
    async fn scripted_exchange_over_loopback_sets_utc() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network_manager, time_manager, None);

        let server_transmit_unix_nanos =
            time_manager.realtime_now().unwrap().as_nanos() + 3_600_000_000_000;
        let (ready, server_task) = spawn_scripted_server(
            move |request| alloc::vec![valid_answer(server_transmit_unix_nanos)(request)],
            1,
        )
        .await;
        ready.wait().await;

        let mut adapter = None;
        let socket = client.ensure_socket(&mut adapter).await.unwrap();
        let exchange = exchange(network_manager, &client, "127.0.0.1", socket)
            .await
            .expect("exchange succeeds");
        let applied = client.apply(&exchange).expect("answer applies");
        server_task.join().await;

        assert_eq!(exchange.result.stratum, 2);
        assert_eq!(exchange.result.leap_indicator, 0);
        // The server is one hour ahead: the RFC 4330 offset says so.
        assert!(
            (exchange.result.offset - 3_600_000_000).abs() < 100_000,
            "offset {} us",
            exchange.result.offset
        );
        assert!(applied.offset_nanos.unwrap() > 3_500_000_000_000);
        let synchronization = time_manager.status().synchronization.expect("synchronized");
        assert_eq!(synchronization.source, time::ClockSource::Network);
        assert_eq!(synchronization.accuracy, Some(applied.accuracy));
        let expected = server_transmit_unix_nanos + exchange.result.roundtrip as i128 * 500;
        let realtime = time_manager.realtime_now().unwrap().as_nanos();
        assert!(
            (realtime - expected).abs() < Duration::from_secs(1).as_nanos() as i128,
            "realtime {realtime} within 1 s of {expected}"
        );
        assert!(exchange.result.roundtrip > 0);
    }

    /// A stray datagram before the real answer (wrong originate timestamp) is
    /// skipped, and the real answer still synchronizes the clock.
    #[task::test]
    async fn a_stray_datagram_before_the_real_answer_is_skipped() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network_manager, time_manager, None);

        let server_transmit_unix_nanos =
            time_manager.realtime_now().unwrap().as_nanos() + 3_600_000_000_000;
        let (ready, server_task) = spawn_scripted_server(
            move |request| {
                let mut forged = valid_answer(server_transmit_unix_nanos)(request);
                // A forged originate timestamp: not what the client sent.
                forged[24..32].copy_from_slice(&ntp_wire(2_000_000_000_000_000_000).to_be_bytes());
                alloc::vec![forged, valid_answer(server_transmit_unix_nanos)(request)]
            },
            1,
        )
        .await;
        ready.wait().await;

        let mut adapter = None;
        let socket = client.ensure_socket(&mut adapter).await.unwrap();
        let exchange = exchange(network_manager, &client, "127.0.0.1", socket)
            .await
            .expect("the real answer is used after the stray one");
        let applied = client.apply(&exchange).expect("answer applies");
        server_task.join().await;

        assert_eq!(exchange.result.stratum, 2);
        assert!(applied.offset_nanos.unwrap() > 3_500_000_000_000);
        assert_eq!(
            time_manager.status().synchronization.map(|s| s.source),
            Some(time::ClockSource::Network)
        );
    }

    /// A kiss of death from the first address is kept even when the second
    /// address stays silent.
    #[task::test]
    async fn a_kiss_of_death_from_the_first_address_is_not_lost() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network_manager, time_manager, None);

        let deny = |request: [u8; 48]| {
            let mut response = [0u8; 48];
            response[0] = 0x24 | (3 << 6);
            response[1] = 0; // stratum 0: kiss of death
            response[12..16].copy_from_slice(b"DENY");
            response[24..32].copy_from_slice(&request[40..48]);
            response[40..48].copy_from_slice(&request[40..48]);
            alloc::vec![response]
        };
        let (ready, server_task) = spawn_scripted_server(deny, 1).await;
        ready.wait().await;

        let mut adapter = None;
        let socket = client.ensure_socket(&mut adapter).await.unwrap();
        // The first address answers DENY; the second one is silent and burns
        // the rest of the shared deadline.
        let addresses = [
            IpAddress::new_ipv4([127, 0, 0, 1]),
            IpAddress::new_ipv4([127, 0, 0, 2]),
        ];
        let result = client.exchange_addresses(&addresses, socket).await;
        assert!(
            matches!(
                result,
                Err(Failure::Protocol(sntpc::Error::KissOfDeath(
                    sntpc::KissOfDeathCode::Deny
                )))
            ),
            "the kiss of death wins over the later timeout: {result:?}"
        );
        server_task.join().await;
    }

    /// An answer that does not echo the request transmit timestamp is rejected
    /// by `sntpc`, and the clock is left alone.
    #[task::test]
    async fn forged_originate_is_rejected() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network_manager, time_manager, None);

        let forged = |request: [u8; 48]| {
            let mut response = valid_answer(2_000_000_000_000_000_000)(request);
            // A forged originate timestamp: not what the client sent.
            response[24..32].copy_from_slice(&ntp_wire(2_000_000_000_000_000_000).to_be_bytes());
            alloc::vec![response]
        };
        let (ready, server_task) = spawn_scripted_server(forged, 1).await;
        ready.wait().await;

        let mut adapter = None;
        let socket = client.ensure_socket(&mut adapter).await.unwrap();
        // The forged answer is skipped like a stray datagram; with no other
        // answer, the exchange ends on the deadline.
        let result = exchange(network_manager, &client, "127.0.0.1", socket).await;
        assert!(
            matches!(result, Err(Failure::TimedOut)),
            "forged answer rejected: {result:?}"
        );
        server_task.join().await;

        assert_eq!(
            time_manager.status().synchronization.map(|s| s.source),
            Some(time::ClockSource::Host),
            "the forged answer did not set the clock: the source stays the host clock"
        );
    }

    /// A kiss-of-death answer (stratum 0, RATE code) surfaces as
    /// `sntpc::Error::KissOfDeath(Rate)` for the service policy to apply.
    #[task::test]
    async fn kiss_of_death_is_surfaced() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network_manager, time_manager, None);

        let kod = |request: [u8; 48]| {
            let mut response = [0u8; 48];
            response[0] = 0x24 | (3 << 6); // leap alarm is allowed in a KoD
            response[1] = 0; // stratum 0: kiss of death
            response[12..16].copy_from_slice(b"RATE"); // reference id is the code
            response[24..32].copy_from_slice(&request[40..48]);
            response[40..48].copy_from_slice(&request[40..48]);
            alloc::vec![response]
        };
        let (ready, server_task) = spawn_scripted_server(kod, 1).await;
        ready.wait().await;

        let mut adapter = None;
        let socket = client.ensure_socket(&mut adapter).await.unwrap();
        let result = exchange(network_manager, &client, "127.0.0.1", socket).await;
        assert!(
            matches!(
                result,
                Err(Failure::Protocol(sntpc::Error::KissOfDeath(
                    sntpc::KissOfDeathCode::Rate
                )))
            ),
            "kiss of death surfaced: {result:?}"
        );
        server_task.join().await;
        assert!(
            time_manager.realtime_now().is_ok(),
            "host clock still there"
        );
    }
}
