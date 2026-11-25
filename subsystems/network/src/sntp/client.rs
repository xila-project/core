//! The SNTP client: the `sntpc` crate does the protocol (RFC 4330 and RFC 5905
//! packet handling, validation, era reconstruction and the four-timestamp
//! offset); this module adapts it to Xila: a [`sntpc::NtpUdpSocket`] over the
//! network subsystem's [`UdpSocket`], a [`sntpc::NtpTimestampGenerator`] over
//! the time manager, and the application of the result to the system UTC
//! clock.

use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use core::time::Duration;

use embassy_futures::select::{Either, select};
use smoltcp::phy::PacketMeta;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use time::{ClockSource, RealtimeSample, Timestamp};

use crate::{
    IntoIoErrorKind, IoErrorKind, IpAddress, Ipv4, Ipv6, Manager, Port, Result, UdpMetadata,
    UdpSocket,
};

/// UDP port of the NTP servers.
pub const SERVER_PORT: u16 = 123;

/// Size of an SNTP packet, in bytes.
pub const PACKET_LENGTH: usize = 48;

/// How many datagrams the socket buffers during one exchange, so that a stray
/// answer cannot overflow the buffer before the real answer is read.
pub const RECEIVE_PACKETS: usize = 4;

/// How long one exchange waits for the server answer in total.
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Answers whose measured round trip exceeds this bound are rejected: the
/// clock cannot be trusted from an answer that old. With the 5 s response
/// timeout this catches answers that arrived almost too late.
pub const MAXIMUM_ROUND_TRIP: Duration = Duration::from_secs(2);

/// After the first network synchronization, a clock step larger than this is
/// only applied when a second answer agrees within
/// [`STEP_CONFIRMATION_TOLERANCE`].
pub const MAXIMUM_CLOCK_STEP: Duration = Duration::from_secs(1000);

/// How close two answers must be to confirm a large clock step.
pub const STEP_CONFIRMATION_TOLERANCE: Duration = Duration::from_secs(5);

const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Why one exchange failed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Failure {
    /// The `sntpc` protocol layer rejected the exchange or the answer, for
    /// example a kiss-of-death packet, or an answer before the platform's
    /// minimum timestamp.
    Protocol(sntpc::Error),
    /// The network or socket layer failed.
    Io(IoErrorKind),
    /// No answer arrived within [`RESPONSE_TIMEOUT`].
    TimedOut,
}

/// What one successful exchange produced: the `sntpc` result and the monotonic
/// instant just after the answer was processed.
#[derive(Clone, Copy, Debug)]
pub struct Exchange {
    pub result: sntpc::NtpResult,
    pub received_monotonic: Duration,
}

/// What applying an exchange changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applied {
    /// How far the new UTC moves the clock, in nanoseconds; `None` when the
    /// clock was unsynchronized before, so there was nothing to compare with.
    pub offset_nanos: Option<i128>,
    /// The offset of the server clock against the local clock, in microseconds,
    /// computed by `sntpc` from all four timestamps as RFC 4330 defines.
    pub server_offset_micros: i64,
    /// The measured round trip.
    pub round_trip: Duration,
    /// The accuracy reported with the sample.
    pub accuracy: Duration,
}

/// A [`sntpc::NtpUdpSocket`] over the network subsystem's [`UdpSocket`].
///
/// Xila's `UdpSocket::write_to` needs exclusive access, so the socket sits
/// behind an asynchronous mutex; `sntpc` calls `send_to` and `recv_from`
/// sequentially, never concurrently. Errors map to [`sntpc::Error`]: a
/// truncated datagram is an incorrect payload, everything else is a network
/// error, and the original [`IoErrorKind`] is kept in `last_io_error` because
/// `sntpc` reports every I/O failure as `Error::Network`. The response source
/// address is checked by `sntpc` itself.
pub struct SntpUdpSocket {
    socket: Mutex<CriticalSectionRawMutex, UdpSocket>,
    last_io_error: Mutex<CriticalSectionRawMutex, Option<IoErrorKind>>,
}

impl SntpUdpSocket {
    pub fn new(socket: UdpSocket) -> Self {
        Self {
            socket: Mutex::new(socket),
            last_io_error: Mutex::new(None),
        }
    }

    /// Drop datagrams that are still queued, for example an answer that
    /// arrived too late for a previous exchange: `sntpc` would reject it on
    /// the originate timestamp check anyway.
    pub async fn discard_pending(&self) {
        let socket = self.socket.lock().await;
        while socket.with(|s| s.can_recv()).await {
            let mut stale = [0u8; PACKET_LENGTH];
            let _ = socket.read_from(&mut stale).await;
        }
    }

    /// Bind a fresh ephemeral source port, so that every exchange sends from a
    /// new port. This also drops queued datagrams.
    pub async fn rebind_ephemeral(&self) -> Result<Port> {
        self.socket.lock().await.rebind_ephemeral().await
    }

    /// The kind of the last socket error, if any.
    pub async fn last_io_error(&self) -> Option<IoErrorKind> {
        *self.last_io_error.lock().await
    }

    /// Record a socket error and map it to the `sntpc` error.
    async fn record_io_error(&self, kind: IoErrorKind) -> sntpc::Error {
        *self.last_io_error.lock().await = Some(kind);
        if kind == IoErrorKind::DatagramTruncated {
            sntpc::Error::IncorrectPayload
        } else {
            sntpc::Error::Network
        }
    }
}

impl sntpc::NtpUdpSocket for SntpUdpSocket {
    async fn send_to(&self, packet: &[u8], destination: SocketAddr) -> sntpc::Result<usize> {
        let mut socket = self.socket.lock().await;
        let metadata = UdpMetadata::new(
            from_core_ip(destination.ip()),
            Port::from_inner(destination.port()),
            None,
            PacketMeta::default(),
        );
        match socket.write_to(packet, &metadata).await {
            Ok(()) => Ok(packet.len()),
            Err(kind) => Err(self.record_io_error(kind).await),
        }
    }

    async fn recv_from(&self, packet: &mut [u8]) -> sntpc::Result<(usize, SocketAddr)> {
        let socket = self.socket.lock().await;
        match socket.read_from(packet).await {
            Ok((size, remote)) => Ok((
                size,
                SocketAddr::new(
                    to_core_ip(remote.remote_address),
                    remote.remote_port.into_inner(),
                ),
            )),
            Err(kind) => Err(self.record_io_error(kind).await),
        }
    }
}

/// A [`sntpc::NtpTimestampGenerator`] over the time manager.
///
/// `init` samples the current UTC estimate. When UTC is unsynchronized the
/// generator falls back to the monotonic clock read as Unix nanoseconds: `sntpc`
/// reconstructs the NTP era from this estimate, so it only has to be within
/// about 68 years of the real time. On Xila targets the runtime seeds UTC from
/// the build date at boot, so this fallback is only a safety net for a manager
/// that was never seeded.
#[derive(Clone, Copy)]
pub struct ManagerTimestampGenerator<'a> {
    time: &'a time::Manager<'a>,
    seconds: u64,
    subsec_micros: u32,
}

impl<'a> ManagerTimestampGenerator<'a> {
    pub fn new(time: &'a time::Manager<'a>) -> Self {
        Self {
            time,
            seconds: 0,
            subsec_micros: 0,
        }
    }
}

impl sntpc::NtpTimestampGenerator for ManagerTimestampGenerator<'_> {
    fn init(&mut self) {
        let nanos = match self.time.realtime_now() {
            Ok(timestamp) => timestamp.as_nanos(),
            Err(_) => self.time.monotonic_now().as_nanos() as i128,
        }
        .clamp(0, u64::MAX as i128);
        self.seconds = u64::try_from(nanos / NANOS_PER_SECOND).unwrap_or(u64::MAX);
        self.subsec_micros = u32::try_from((nanos % NANOS_PER_SECOND) / 1_000).unwrap_or(0);
    }

    fn timestamp_sec(&self) -> u64 {
        self.seconds
    }

    fn timestamp_subsec_micros(&self) -> u32 {
        self.subsec_micros
    }

    fn precision(&self) -> i8 {
        // log2 of the monotonic resolution in seconds, floored: 1 microsecond
        // gives about -20.
        let nanos = self.time.monotonic_resolution().as_nanos().max(1);
        (127 - nanos.leading_zeros()) as i8 - 30
    }
}

/// A client bound to the network and time subsystems.
pub struct Client<'a> {
    network: &'a Manager,
    time: &'a time::Manager<'a>,
    minimum_timestamp: Option<Timestamp>,
}

impl<'a> Client<'a> {
    /// `minimum_timestamp` is the platform's floor for UTC, for example the
    /// build date: answers before it are rejected.
    pub fn new(
        network: &'a Manager,
        time: &'a time::Manager<'a>,
        minimum_timestamp: Option<Timestamp>,
    ) -> Self {
        Self {
            network,
            time,
            minimum_timestamp,
        }
    }

    /// Return the shared socket, creating and binding it on first use. The
    /// socket buffers are the only per-exchange structure.
    pub async fn ensure_socket<'b>(
        &self,
        socket: &'b mut Option<SntpUdpSocket>,
    ) -> Result<&'b mut SntpUdpSocket> {
        if socket.is_none() {
            let mut created = self
                .network
                .new_udp_socket(
                    PACKET_LENGTH,
                    PACKET_LENGTH * RECEIVE_PACKETS,
                    RECEIVE_PACKETS,
                    4,
                    None,
                )
                .await?;
            created.bind_ephemeral().await?;
            *socket = Some(SntpUdpSocket::new(created));
        }
        Ok(socket.as_mut().expect("socket was just created"))
    }

    /// Exchange with each address in turn until one answers. One deadline
    /// covers all the addresses of the name. A kiss-of-death answer is kept in
    /// preference to the errors of the later addresses.
    pub async fn exchange_addresses(
        &self,
        addresses: &[IpAddress],
        socket: &SntpUdpSocket,
    ) -> core::result::Result<Exchange, Failure> {
        let deadline = self.time.monotonic_now() + RESPONSE_TIMEOUT;
        let mut last_error = Failure::Protocol(sntpc::Error::Network);
        let mut kiss_of_death: Option<sntpc::Error> = None;
        for address in addresses {
            let destination = SocketAddr::new(to_core_ip(address.clone()), SERVER_PORT);
            match self.exchange_one(destination, socket, deadline).await {
                Ok(exchange) => return Ok(exchange),
                Err(Failure::Protocol(error @ sntpc::Error::KissOfDeath(_))) => {
                    if kiss_of_death.is_none() {
                        kiss_of_death = Some(error);
                    }
                    last_error = Failure::Protocol(error);
                }
                Err(error) => last_error = error,
            }
        }
        Err(match kiss_of_death {
            Some(error) => Failure::Protocol(error),
            None => last_error,
        })
    }

    /// One request/response with one server address, bounded by `deadline`.
    /// Stray datagrams (a wrong source address, a wrong originate timestamp
    /// or an oversized payload) are skipped until the deadline, so that one
    /// stray packet does not abort the exchange.
    async fn exchange_one(
        &self,
        destination: SocketAddr,
        socket: &SntpUdpSocket,
        deadline: Duration,
    ) -> core::result::Result<Exchange, Failure> {
        socket.discard_pending().await;
        // A fresh source port per exchange (the stack's port counter is
        // sequential). A failure leaves the socket closed: report it so the
        // caller can rebuild the socket.
        socket.rebind_ephemeral().await.map_err(Failure::Io)?;
        let now = self.time.monotonic_now();
        if now >= deadline {
            return Err(Failure::TimedOut);
        }
        let context = sntpc::NtpContext::new(ManagerTimestampGenerator::new(self.time));
        let request = sntpc::sntp_send_request(destination, socket, context);
        let sent = match select(request, task::sleep(deadline - now)).await {
            Either::First(Ok(sent)) => sent,
            Either::First(Err(error)) => return Err(self.translate(socket, error).await),
            Either::Second(()) => return Err(Failure::TimedOut),
        };
        loop {
            let now = self.time.monotonic_now();
            if now >= deadline {
                return Err(Failure::TimedOut);
            }
            let context = sntpc::NtpContext::new(ManagerTimestampGenerator::new(self.time));
            let answer = sntpc::sntp_process_response(destination, socket, context, sent);
            match select(answer, task::sleep(deadline - now)).await {
                Either::First(Ok(result)) => {
                    return Ok(Exchange {
                        result,
                        received_monotonic: self.time.monotonic_now(),
                    });
                }
                Either::First(Err(error)) => {
                    if matches!(
                        error,
                        sntpc::Error::ResponseAddressMismatch
                            | sntpc::Error::IncorrectOriginTimestamp
                            | sntpc::Error::IncorrectPayload
                    ) {
                        // A stray datagram: read the next one.
                        continue;
                    }
                    return Err(self.translate(socket, error).await);
                }
                Either::Second(()) => return Err(Failure::TimedOut),
            }
        }
    }

    /// Map a `sntpc` error to a [`Failure`], restoring the socket error kind
    /// that `sntpc` flattened into `Error::Network`.
    async fn translate(&self, socket: &SntpUdpSocket, error: sntpc::Error) -> Failure {
        if error == sntpc::Error::Network
            && let Some(kind) = socket.last_io_error().await
        {
            return Failure::Io(kind);
        }
        Failure::Protocol(error)
    }

    /// The UTC value an exchange would set, without setting the clock.
    pub fn candidate(&self, exchange: &Exchange) -> core::result::Result<Timestamp, Failure> {
        candidate(self.time, exchange, self.minimum_timestamp)
    }

    /// Apply a successful exchange to the system UTC clock. See [`apply`].
    pub fn apply(&self, exchange: &Exchange) -> core::result::Result<Applied, Failure> {
        apply(self.time, exchange, self.minimum_timestamp)
    }
}

/// The UTC value of an exchange: the server transmit time plus half the round
/// trip, carried over to now. The answer is rejected when the round trip is
/// too long or the value is before the platform's minimum timestamp.
pub fn candidate(
    time: &time::Manager<'_>,
    exchange: &Exchange,
    minimum_timestamp: Option<Timestamp>,
) -> core::result::Result<Timestamp, Failure> {
    let result = &exchange.result;
    let round_trip = Duration::from_micros(result.roundtrip);
    if round_trip > MAXIMUM_ROUND_TRIP {
        return Err(Failure::Io(IoErrorKind::TimedOut));
    }
    let server_nanos = result.seconds as i128 * NANOS_PER_SECOND
        + i128::from(sntpc::fraction_to_nanoseconds(result.seconds_fraction));
    // The sample is anchored when `set_realtime` runs, a little after the
    // answer was processed: carry the elapsed time over.
    let since_receive = (time.monotonic_now() - exchange.received_monotonic).as_nanos() as i128;
    let timestamp =
        Timestamp::from_nanos(server_nanos + round_trip.as_nanos() as i128 / 2 + since_receive);
    if let Some(minimum) = minimum_timestamp
        && timestamp < minimum
    {
        return Err(Failure::Protocol(sntpc::Error::InvalidTimestamp));
    }
    Ok(timestamp)
}

/// Set UTC from a `sntpc` result: the server transmit time plus half the round
/// trip, with [`ClockSource::Network`]. The accuracy estimate is half the round
/// trip plus the server precision, the server root distance and the local clock
/// resolution. Returns what changed.
pub fn apply(
    time: &time::Manager<'_>,
    exchange: &Exchange,
    minimum_timestamp: Option<Timestamp>,
) -> core::result::Result<Applied, Failure> {
    let timestamp = candidate(time, exchange, minimum_timestamp)?;
    let resolution = time.monotonic_resolution();
    let accuracy = accuracy(&exchange.result, resolution);
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
    .map_err(|error| Failure::Io(error.into_io_error_kind()))?;
    Ok(Applied {
        offset_nanos,
        server_offset_micros: exchange.result.offset,
        round_trip: Duration::from_micros(exchange.result.roundtrip),
        accuracy,
    })
}

/// The accuracy reported with a sample: half the round trip, the server
/// precision, the server root distance and the local clock resolution.
fn accuracy(result: &sntpc::NtpResult, client_resolution: Duration) -> Duration {
    let precision_nanos = precision_to_nanos(result.precision);
    let root_distance_nanos = short_format_to_nanos(result.root_delay / 2)
        + short_format_to_nanos(result.root_dispersion);
    let total = u128::from(result.roundtrip) * 500
        + precision_nanos
        + root_distance_nanos
        + client_resolution.as_nanos();
    Duration::from_nanos(total.min(u64::MAX as u128) as u64)
}

/// 2^precision seconds, in nanoseconds, floored at zero.
fn precision_to_nanos(precision: i8) -> u128 {
    if precision >= 0 {
        1_000_000_000u128 << precision.min(38)
    } else {
        1_000_000_000u128 >> (-precision).min(63)
    }
}

/// An NTP short format (16.16 fixed point) value, in nanoseconds.
fn short_format_to_nanos(value: u32) -> u128 {
    u128::from(value) * 1_000_000_000 / 65_536
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
            | IoErrorKind::InvalidState
    )
}

fn to_core_ip(address: IpAddress) -> IpAddr {
    match address {
        IpAddress::IPv4(value) => IpAddr::V4(Ipv4Addr::from(value.into_inner())),
        IpAddress::IPv6(value) => IpAddr::V6(Ipv6Addr::from(value.into_inner())),
    }
}

fn from_core_ip(address: IpAddr) -> IpAddress {
    match address {
        IpAddr::V4(value) => IpAddress::IPv4(Ipv4::from_inner(value.octets())),
        IpAddr::V6(value) => IpAddress::IPv6(Ipv6::from_inner(value.octets())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sntpc::NtpTimestampGenerator;

    /// 2026-01-01 00:00:00 UTC, in Unix seconds.
    const UNIX_2026: u64 = 1_767_225_600;

    fn result(roundtrip_us: u64, offset_us: i64) -> sntpc::NtpResult {
        sntpc::NtpResult::new(
            UNIX_2026,
            1 << 31, // half a second
            roundtrip_us,
            offset_us,
            2,
            -20,
            0,
            0,
            0x0000_1000, // root dispersion: 1/16 second
            [b'G', b'P', b'S', 0],
            0,
            6,
            0,
        )
    }

    #[test]
    fn ip_addresses_convert_to_and_from_core_net() {
        let v4 = IpAddress::new_ipv4([192, 0, 2, 1]);
        assert_eq!(
            to_core_ip(v4.clone()),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1))
        );
        assert_eq!(from_core_ip(to_core_ip(v4.clone())), v4);
        let v6 = IpAddress::new_ipv6([0x2001, 0xdb8, 0, 0, 0, 0, 0, 1]);
        assert_eq!(from_core_ip(to_core_ip(v6.clone())), v6);
    }

    #[test]
    fn invalidates_socket_only_for_network_failures() {
        assert!(invalidates_socket(IoErrorKind::NetworkDown));
        assert!(invalidates_socket(IoErrorKind::NetworkUnreachable));
        assert!(invalidates_socket(IoErrorKind::HostUnreachable));
        assert!(invalidates_socket(IoErrorKind::DeviceUnavailable));
        assert!(invalidates_socket(IoErrorKind::InvalidState));
        assert!(!invalidates_socket(IoErrorKind::TimedOut));
        assert!(!invalidates_socket(IoErrorKind::InvalidData));
    }

    #[test]
    fn accuracy_combines_round_trip_precision_root_distance_and_resolution() {
        let resolution = Duration::from_micros(1);
        // Round trip 20 ms, server precision 2^-20 s (about 1 us), root
        // dispersion 1/16 s: 10 ms + 1 us + 62.5 ms + 1 us.
        let expected = Duration::from_nanos(10_000_000 + 953 + 62_500_000 + 1_000);
        assert_eq!(accuracy(&result(20_000, 0), resolution), expected);
    }

    #[test]
    fn precision_to_nanos_scales_with_log2_seconds() {
        assert_eq!(precision_to_nanos(0), 1_000_000_000);
        assert_eq!(precision_to_nanos(-10), 976_562);
        assert_eq!(precision_to_nanos(-20), 953);
        assert_eq!(precision_to_nanos(-40), 0);
    }

    #[test]
    fn short_format_to_nanos_decodes_16_16_fixed_point() {
        assert_eq!(short_format_to_nanos(0), 0);
        assert_eq!(short_format_to_nanos(1 << 16), 1_000_000_000);
        assert_eq!(short_format_to_nanos(0x0000_8000), 500_000_000);
    }

    #[test]
    fn apply_sets_utc_to_server_transmit_plus_half_the_round_trip() {
        let time_manager = time::Manager::unsynchronized();
        let received = time_manager.monotonic_now();
        let exchange = Exchange {
            result: result(20_000, 3_600_000_000),
            received_monotonic: received,
        };
        let applied = apply(&time_manager, &exchange, None).unwrap();
        let synchronization = time_manager.status().synchronization.expect("synchronized");
        assert_eq!(synchronization.source, ClockSource::Network);
        assert_eq!(applied.round_trip, Duration::from_millis(20));
        assert_eq!(applied.server_offset_micros, 3_600_000_000);
        assert_eq!(applied.offset_nanos, None, "was unsynchronized before");
        let expected = UNIX_2026 as i128 * NANOS_PER_SECOND + 500_000_000 + 10_000_000;
        let realtime = time_manager.realtime_now().unwrap().as_nanos();
        assert!(
            (realtime - expected).abs() < 1_000_000_000,
            "realtime {realtime} within 1 s of {expected}"
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
        let exchange = Exchange {
            result: result(0, 0),
            received_monotonic: time_manager.monotonic_now(),
        };
        let applied = apply(&time_manager, &exchange, None).unwrap();
        let offset = applied.offset_nanos.expect("clock was set before");
        let expected =
            UNIX_2026 as i128 * NANOS_PER_SECOND + 500_000_000 - 1_700_000_000_000_000_000;
        assert!(
            (offset - expected).abs() < 1_000_000_000,
            "offset {offset} within 1 s of {expected}"
        );
    }

    #[test]
    fn apply_rejects_an_overly_long_round_trip() {
        let time_manager = time::Manager::unsynchronized();
        let exchange = Exchange {
            result: result(60_000_000, 0),
            received_monotonic: time_manager.monotonic_now(),
        };
        assert_eq!(
            apply(&time_manager, &exchange, None),
            Err(Failure::Io(IoErrorKind::TimedOut))
        );
        assert!(
            time_manager.realtime_now().is_err(),
            "clock stays unsynchronized"
        );
    }

    #[test]
    fn apply_rejects_an_answer_before_the_minimum_timestamp() {
        let time_manager = time::Manager::unsynchronized();
        let minimum = Timestamp::from_nanos((UNIX_2026 as i128 + 3600) * NANOS_PER_SECOND);
        let exchange = Exchange {
            result: result(1_000, 0),
            received_monotonic: time_manager.monotonic_now(),
        };
        assert_eq!(
            apply(&time_manager, &exchange, Some(minimum)),
            Err(Failure::Protocol(sntpc::Error::InvalidTimestamp))
        );
        assert!(time_manager.realtime_now().is_err());
    }

    #[test]
    fn the_timestamp_generator_samples_the_manager() {
        let time_manager = time::Manager::unsynchronized();
        time_manager
            .set_realtime(time::RealtimeSample {
                timestamp: Timestamp::from_nanos(
                    UNIX_2026 as i128 * NANOS_PER_SECOND + 250_000_000,
                ),
                source: ClockSource::Host,
                resolution: Duration::from_millis(1),
                accuracy: None,
            })
            .unwrap();
        let mut generator = ManagerTimestampGenerator::new(&time_manager);
        generator.init();
        assert_eq!(generator.timestamp_sec(), UNIX_2026);
        // The realtime clock can advance between the sample above and init.
        let subsec = generator.timestamp_subsec_micros();
        assert!(
            subsec.abs_diff(250_000) <= 10_000,
            "subsecond {subsec} within 10 ms of 250_000"
        );
        assert!(
            generator.precision() <= -10,
            "millisecond resolution or finer"
        );
    }

    #[test]
    fn the_timestamp_generator_falls_back_to_the_monotonic_clock() {
        let time_manager = time::Manager::unsynchronized();
        let mut generator = ManagerTimestampGenerator::new(&time_manager);
        generator.init();
        // The monotonic clock read as Unix seconds: a small non-negative value.
        assert!(generator.timestamp_sec() < 100_000_000);
    }
}
