//! The SNTP client: one request/answer exchange over a UDP socket, and the
//! application of a validated answer to the system UTC clock.

use core::future::poll_fn;
use core::time::Duration;

use embassy_futures::select::{Either, select};
use smoltcp::phy::PacketMeta;
use time::{ClockSource, RealtimeSample, Timestamp};

use crate::{
    DnsQueryKind, IntoIoErrorKind, IoErrorKind, IpAddress, Manager, Port, Result, UdpMetadata,
    UdpSocket,
    sntp::packet::{
        ExchangeTiming, PACKET_LENGTH, SERVER_PORT, build_request, ntp_from_unix_nanos,
        parse_response,
    },
};

/// How long to wait for a server answer before giving up on the exchange.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Answers whose measured round trip exceeds this bound are rejected: the
/// clock cannot be trusted from an answer that old.
pub const MAXIMUM_ROUND_TRIP: Duration = Duration::from_secs(10);

/// What one successful exchange changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applied {
    /// How far the new UTC moves the clock, in nanoseconds; `None` when the
    /// clock was unsynchronized before, so there was nothing to compare with.
    pub offset_nanos: Option<i128>,
    /// The measured round trip.
    pub round_trip: Duration,
    /// The accuracy reported with the sample.
    pub accuracy: Duration,
}

/// A client bound to the network and time subsystems.
pub struct Client<'a> {
    network: &'a Manager,
    time: &'a time::Manager<'a>,
}

impl<'a> Client<'a> {
    pub fn new(network: &'a Manager, time: &'a time::Manager<'a>) -> Self {
        Self { network, time }
    }

    /// Return the shared socket, creating and binding it on first use. The
    /// socket buffers are the only allocation of an exchange.
    pub async fn ensure_socket<'b>(
        &self,
        socket: &'b mut Option<UdpSocket>,
    ) -> Result<&'b mut UdpSocket> {
        if socket.is_none() {
            let mut created = self
                .network
                .new_udp_socket(PACKET_LENGTH, PACKET_LENGTH, 4, 4, None)
                .await?;
            created.bind_ephemeral().await?;
            *socket = Some(created);
        }
        Ok(socket.as_mut().expect("socket was just created"))
    }

    /// Query one server by name or address. Resolves the name, sends one
    /// request to UDP port 123 and waits up to [`RESPONSE_TIMEOUT`] for the
    /// answer. Returns the timing of the exchange when the answer passed
    /// validation.
    pub async fn exchange(&self, server: &str, socket: &mut UdpSocket) -> Result<ExchangeTiming> {
        let addresses = self
            .network
            .resolve(server, DnsQueryKind::A | DnsQueryKind::Aaaa, true, None)
            .await
            .map_err(|error| error.kind())?;

        let mut last_error = IoErrorKind::HostUnreachable;
        for address in &addresses {
            match self.exchange_with_address(address.clone(), socket).await {
                Ok(timing) => return Ok(timing),
                Err(error) => last_error = error,
            }
        }
        Err(last_error)
    }

    async fn exchange_with_address(
        &self,
        address: IpAddress,
        socket: &mut UdpSocket,
    ) -> Result<ExchangeTiming> {
        // Best estimate of "now", used both for the request timestamp and to
        // resolve the era of the answer. When UTC is not synchronized yet, the
        // monotonic clock stands in: any estimate within about 68 years of the
        // real time resolves the era correctly.
        let estimate_nanos = self
            .time
            .realtime_now()
            .map(|timestamp| timestamp.as_nanos())
            .unwrap_or_else(|_| self.time.monotonic_now().as_nanos() as i128);

        let transmit = ntp_from_unix_nanos(estimate_nanos);
        let mut request = [0u8; PACKET_LENGTH];
        build_request(transmit, &mut request);

        // Drop answers that arrived too late for a previous exchange: the
        // originate check would reject them anyway.
        while socket.with(|s| s.can_recv()).await {
            let mut stale = [0u8; PACKET_LENGTH];
            let _ = socket.read_from(&mut stale).await;
        }

        let sent_monotonic = self.time.monotonic_now();
        let metadata = UdpMetadata::new(
            address.clone(),
            Port::from_inner(SERVER_PORT),
            None,
            PacketMeta::default(),
        );
        socket.write_to(&request, &metadata).await?;

        let mut buffer = [0u8; PACKET_LENGTH];
        let receive = poll_fn(|context| socket.poll_receive_from(context, &mut buffer));
        match select(receive, task::sleep(RESPONSE_TIMEOUT)).await {
            Either::First(result) => {
                let (size, remote) = result?;
                let received_monotonic = self.time.monotonic_now();
                // Only the answer of the queried server counts.
                if remote.remote_port.into_inner() != SERVER_PORT
                    || remote.remote_address != address
                {
                    return Err(IoErrorKind::InvalidData);
                }
                let response = parse_response(&buffer[..size], transmit, estimate_nanos)
                    .map_err(|_| IoErrorKind::InvalidData)?;
                Ok(ExchangeTiming {
                    server_transmit_unix_nanos: response.transmit_unix_nanos,
                    sent_monotonic,
                    received_monotonic,
                })
            }
            Either::Second(()) => Err(IoErrorKind::TimedOut),
        }
    }

    /// Apply a validated exchange to the system UTC clock. See [`apply`].
    pub fn apply(&self, timing: &ExchangeTiming) -> Result<Applied> {
        apply(self.time, timing)
    }

    /// True when the error means the socket should be rebuilt, for example
    /// because the interface went away.
    pub fn invalidates_socket(error: IoErrorKind) -> bool {
        invalidates_socket(error)
    }
}

/// Set UTC from a validated exchange: the server transmit time plus half the
/// round trip, with [`ClockSource::Network`] and the estimated accuracy (half
/// the round trip plus the clock resolution). Returns what changed.
pub fn apply(time: &time::Manager<'_>, timing: &ExchangeTiming) -> Result<Applied> {
    let utc_at_receive = timing
        .utc_at_receive_nanos(MAXIMUM_ROUND_TRIP)
        .map_err(|_| IoErrorKind::TimedOut)?;
    let resolution = time.monotonic_resolution();
    let round_trip = timing.round_trip();
    // The sample is anchored when `set_realtime` runs, a little after the
    // receive instant: carry the elapsed time over.
    let since_receive = (time.monotonic_now() - timing.received_monotonic).as_nanos();
    let timestamp = Timestamp::from_nanos(utc_at_receive + since_receive as i128);
    let accuracy = round_trip / 2 + resolution;
    let offset_nanos = time
        .realtime_now()
        .ok()
        .map(|previous| timestamp.as_nanos() - previous.as_nanos());
    time.set_realtime(RealtimeSample {
        timestamp,
        source: ClockSource::Network,
        resolution,
        accuracy: Some(accuracy),
    })
    .map_err(|error| error.into_io_error_kind())?;
    Ok(Applied {
        offset_nanos,
        round_trip,
        accuracy,
    })
}

/// True when the error means the shared socket should be rebuilt, for example
/// because the interface went away.
pub fn invalidates_socket(error: IoErrorKind) -> bool {
    matches!(
        error,
        IoErrorKind::NetworkDown
            | IoErrorKind::NetworkUnreachable
            | IoErrorKind::HostUnreachable
            | IoErrorKind::DeviceUnavailable
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidates_socket_only_for_network_failures() {
        assert!(invalidates_socket(IoErrorKind::NetworkDown));
        assert!(invalidates_socket(IoErrorKind::NetworkUnreachable));
        assert!(invalidates_socket(IoErrorKind::HostUnreachable));
        assert!(invalidates_socket(IoErrorKind::DeviceUnavailable));
        assert!(!invalidates_socket(IoErrorKind::TimedOut));
        assert!(!invalidates_socket(IoErrorKind::InvalidData));
    }

    #[test]
    fn apply_sets_utc_to_transmit_plus_half_the_round_trip() {
        let time_manager = time::Manager::unsynchronized();
        let sent = time_manager.monotonic_now();
        std::thread::sleep(Duration::from_millis(20));
        let received = time_manager.monotonic_now();
        let timing = ExchangeTiming {
            server_transmit_unix_nanos: 1_800_000_000_000_000_000,
            sent_monotonic: sent,
            received_monotonic: received,
        };
        let applied = apply(&time_manager, &timing).unwrap();
        let synchronization = time_manager.status().synchronization.expect("synchronized");
        assert_eq!(synchronization.source, ClockSource::Network);
        assert!(applied.round_trip >= Duration::from_millis(20));
        assert!(
            applied.accuracy == applied.round_trip / 2 + time_manager.monotonic_resolution(),
            "accuracy is half the round trip plus the clock resolution"
        );
        assert_eq!(applied.offset_nanos, None, "was unsynchronized before");
        let realtime = time_manager.realtime_now().unwrap();
        let expected = 1_800_000_000_000_000_000 + timing.round_trip().as_nanos() as i128 / 2;
        assert!(
            (realtime.as_nanos() - expected).abs() < 1_000_000_000,
            "realtime {} within 1 s of {expected}",
            realtime.as_nanos()
        );
    }

    #[test]
    fn apply_reports_the_offset_against_the_previous_clock() {
        let time_manager = time::Manager::unsynchronized();
        time_manager
            .set_realtime(time::RealtimeSample {
                timestamp: Timestamp::from_nanos(1_700_000_000_000_000_000),
                source: ClockSource::Manual,
                resolution: Duration::from_secs(1),
                accuracy: None,
            })
            .unwrap();
        let sent = time_manager.monotonic_now();
        let timing = ExchangeTiming {
            server_transmit_unix_nanos: 1_800_000_000_000_000_000,
            sent_monotonic: sent,
            received_monotonic: sent,
        };
        let applied = apply(&time_manager, &timing).unwrap();
        let offset = applied.offset_nanos.expect("clock was set before");
        assert!(
            (offset - 100_000_000_000_000_000).abs() < 1_000_000_000,
            "offset {offset} within 1 s of 100 s"
        );
    }

    #[test]
    fn apply_rejects_an_overly_long_round_trip() {
        let time_manager = time::Manager::unsynchronized();
        let timing = ExchangeTiming {
            server_transmit_unix_nanos: 1_800_000_000_000_000_000,
            sent_monotonic: Duration::ZERO,
            received_monotonic: Duration::from_secs(60),
        };
        assert_eq!(apply(&time_manager, &timing), Err(IoErrorKind::TimedOut));
        assert!(
            time_manager.realtime_now().is_err(),
            "clock stays unsynchronized"
        );
    }
}
