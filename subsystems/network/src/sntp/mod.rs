//! SNTP client (RFC 4330, version 4 over UDP port 123): sets the system UTC
//! clock from the network through [`time::Manager::set_realtime`] with
//! [`time::ClockSource::Network`].
//!
//! The client lives in the network subsystem because it is a network service
//! owned by that runtime: it uses [`Manager::resolve`] and
//! [`Manager::new_udp_socket`] against the same stacks, and needs the task
//! spawner, the virtual file system (for `/configuration/system/sntp.json`)
//! and the time manager, which are already its dependencies. The packet rules
//! in [`packet`] are pure and host-testable; one exchange allocates nothing
//! beyond its UDP socket and uses one 48-byte buffer.
//!
//! [`Service::start`] spawns the synchronization task: it waits until an
//! interface has an address, queries the configured servers (default
//! `pool.ntp.org`), then resynchronizes periodically with a bounded
//! exponential backoff on failure.

mod client;
mod configuration;
mod packet;
mod service;

pub use client::{Applied, Client, MAXIMUM_ROUND_TRIP, RESPONSE_TIMEOUT, apply};
pub use configuration::{
    CONFIGURATION_PATH, Configuration, ConfigurationError, DEFAULT_RESYNC_INTERVAL, DEFAULT_SERVER,
    ServerList, ServerName, load,
};
pub use packet::{
    ExchangeTiming, NtpTimestamp, PACKET_LENGTH, Response, SERVER_PORT, VERSION, ValidationError,
    build_request, ntp_from_unix_nanos, parse_response, unix_nanos_from_ntp,
};
pub use service::{
    FAILURE_LOG_STRIDE, INITIAL_RETRY_DELAY, MAXIMUM_RETRY_DELAY, NETWORK_POLL_INTERVAL,
    REPEATED_FAILURE_LOG_THRESHOLD, Service, retry_delay,
};

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use core::time::Duration;

    use smoltcp::phy::PacketMeta;
    use synchronization::{
        Arc, blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal,
    };

    use super::packet::{
        PACKET_LENGTH, SERVER_PORT, VERSION, header, ntp_from_unix_nanos, read_timestamp,
        write_timestamp,
    };
    use super::{Client, MAXIMUM_ROUND_TRIP};
    use crate::tests::{initialize, task_manager};
    use crate::{IoErrorKind, Port, UdpMetadata};

    static TEST_MUTEX: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

    fn host_time_manager() -> &'static time::Manager<'static> {
        Box::leak(Box::new(
            time::Manager::new(&testing_support::TimeDevice).unwrap(),
        ))
    }

    /// Spawn a scripted server on 127.0.0.1:123 that reads one request and
    /// answers it with `answer`, built from the request's transmit timestamp
    /// unless `forge_originate` replaces it.
    async fn spawn_scripted_server(
        server_transmit_unix_nanos: i128,
        forge_originate: bool,
    ) -> (
        synchronization::Arc<Signal<CriticalSectionRawMutex, ()>>,
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
                    let mut buffer = [0u8; 64];
                    let (size, remote) = server_socket
                        .read_from(&mut buffer)
                        .await
                        .expect("request received");
                    assert_eq!(size, PACKET_LENGTH, "request is 48 bytes");
                    assert_eq!(buffer[0], 0x23, "leap 0, version 4, mode 3");
                    let mut originate = read_timestamp(&buffer[40..48]);
                    if forge_originate {
                        originate = ntp_from_unix_nanos(server_transmit_unix_nanos);
                    }

                    let transmit = ntp_from_unix_nanos(server_transmit_unix_nanos);
                    let mut response = [0u8; PACKET_LENGTH];
                    response[0] = header(0, VERSION, 4);
                    response[1] = 2;
                    write_timestamp(&mut response[24..32], originate);
                    write_timestamp(&mut response[32..40], transmit);
                    write_timestamp(&mut response[40..48], transmit);
                    let metadata = UdpMetadata::new(
                        remote.remote_address,
                        remote.remote_port,
                        None,
                        PacketMeta::default(),
                    );
                    server_socket
                        .write_to(&response, &metadata)
                        .await
                        .expect("answer sent");
                    // `close` drops a queued packet, and `flush` only checks
                    // capacity: give the runner time to transmit the answer.
                    task::sleep(Duration::from_millis(100)).await;
                    server_socket.close().await;
                },
            )
            .await
            .unwrap();
        (ready, server_task)
    }

    /// A scripted SNTP server on the loopback answers one request with a
    /// transmit time one hour ahead of the host clock, and the client sets UTC
    /// from it.
    #[task::test]
    async fn scripted_exchange_over_loopback_sets_utc() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let client = Client::new(network_manager, time_manager);

        let server_transmit_unix_nanos =
            time_manager.realtime_now().unwrap().as_nanos() + 3_600_000_000_000;
        let (ready, server_task) = spawn_scripted_server(server_transmit_unix_nanos, false).await;
        ready.wait().await;

        let mut socket = None;
        let timing = {
            let shared = client.ensure_socket(&mut socket).await.unwrap();
            client
                .exchange("127.0.0.1", shared)
                .await
                .expect("exchange succeeds")
        };
        let applied = client.apply(&timing).expect("answer applies");
        server_task.join().await;

        assert!(
            applied.offset_nanos.unwrap() > 3_500_000_000_000,
            "the clock moves about one hour forward: {:?}",
            applied.offset_nanos
        );
        let synchronization = time_manager.status().synchronization.expect("synchronized");
        assert_eq!(synchronization.source, time::ClockSource::Network);
        assert_eq!(synchronization.accuracy, Some(applied.accuracy));
        let expected = server_transmit_unix_nanos + timing.round_trip().as_nanos() as i128 / 2;
        let realtime = time_manager.realtime_now().unwrap().as_nanos();
        assert!(
            (realtime - expected).abs() < Duration::from_secs(1).as_nanos() as i128,
            "realtime {realtime} within 1 s of {expected}"
        );
        assert!(timing.round_trip() <= MAXIMUM_ROUND_TRIP);
    }

    /// An answer that does not echo the request transmit timestamp is
    /// rejected, and the clock is left alone.
    #[task::test]
    async fn forged_originate_is_rejected() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let time_manager = host_time_manager();
        let before = time_manager.realtime_now().unwrap().as_nanos();
        let client = Client::new(network_manager, time_manager);

        let (ready, server_task) = spawn_scripted_server(2_000_000_000_000_000_000, true).await;
        ready.wait().await;

        let mut socket = None;
        let shared = client.ensure_socket(&mut socket).await.unwrap();
        let result = client.exchange("127.0.0.1", shared).await;
        assert!(
            matches!(result, Err(IoErrorKind::InvalidData)),
            "forged answer rejected: {result:?}"
        );
        server_task.join().await;

        let after = time_manager.realtime_now().unwrap().as_nanos();
        assert!(
            (after - before).abs() < Duration::from_secs(1).as_nanos() as i128,
            "the clock did not move"
        );
        assert_eq!(
            time_manager.status().synchronization.map(|s| s.source),
            Some(time::ClockSource::Host),
            "the source stays the host clock"
        );
    }
}
