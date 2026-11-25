//! SNTP version 4 packet logic (RFC 4330): building the client request and
//! validating the server answer.
//!
//! This module is pure: no I/O, no allocation and one fixed 48-byte buffer, so
//! it is host-testable and fits targets with little RAM.

use core::time::Duration;

/// Size of the fixed part of an SNTP packet, in bytes.
pub const PACKET_LENGTH: usize = 48;

/// UDP port of the NTP servers.
pub const SERVER_PORT: u16 = 123;

/// SNTP protocol version implemented by this client.
pub const VERSION: u8 = 4;

/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch (1970-01-01).
pub const NTP_UNIX_OFFSET_SECONDS: i64 = 2_208_988_800;

const LEAP_INDICATOR_ALARM: u8 = 3;
const MODE_CLIENT: u8 = 3;
const MODE_SERVER: u8 = 4;
const STRATUM_MINIMUM: u8 = 1;
const STRATUM_MAXIMUM: u8 = 15;
const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// The first byte of a packet: leap indicator, version and mode.
pub const fn header(leap_indicator: u8, version: u8, mode: u8) -> u8 {
    (leap_indicator << 6) | (version << 3) | mode
}

/// A 64-bit NTP timestamp: 32 bits of seconds since 1900 plus a 32-bit fraction
/// of a second. The seconds field alone does not say which 136-year era the
/// value belongs to; see [`unix_nanos_from_ntp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NtpTimestamp {
    pub seconds: u32,
    pub fraction: u32,
}

impl NtpTimestamp {
    pub const ZERO: Self = Self {
        seconds: 0,
        fraction: 0,
    };

    pub const fn is_zero(self) -> bool {
        self.seconds == 0 && self.fraction == 0
    }
}

/// Encode Unix nanoseconds as an NTP timestamp. Only the low 32 bits of the NTP
/// seconds are kept: the era is chosen by the reader, against its own estimate
/// of the current time.
pub fn ntp_from_unix_nanos(unix_nanos: i128) -> NtpTimestamp {
    let seconds = unix_nanos.div_euclid(NANOS_PER_SECOND);
    let subsecond = unix_nanos.rem_euclid(NANOS_PER_SECOND);
    let ntp_seconds = seconds + NTP_UNIX_OFFSET_SECONDS as i128;
    NtpTimestamp {
        seconds: ntp_seconds.rem_euclid(1i128 << 32) as u32,
        fraction: ((subsecond * (1i128 << 32)) / NANOS_PER_SECOND) as u32,
    }
}

/// Decode an NTP timestamp as Unix nanoseconds.
///
/// The 32-bit seconds field wraps in 2036, so the era is the one that places
/// the result closest to `estimate_unix_nanos`, the client's own estimate of
/// the current time. The estimate only has to be right within about 68 years
/// for the era to resolve correctly.
pub fn unix_nanos_from_ntp(timestamp: NtpTimestamp, estimate_unix_nanos: i128) -> i128 {
    let estimate_seconds = estimate_unix_nanos
        .div_euclid(NANOS_PER_SECOND)
        .clamp(i64::MIN as i128, i64::MAX as i128) as i64;
    let estimate_ntp_seconds = estimate_seconds + NTP_UNIX_OFFSET_SECONDS;
    let delta = estimate_ntp_seconds - timestamp.seconds as i64;
    // Round to the nearest era: floor((delta + 2^31) / 2^32).
    let era = (delta + (1i64 << 31)).div_euclid(1i64 << 32);
    let ntp_seconds = era * (1i64 << 32) + timestamp.seconds as i64;
    let unix_seconds = ntp_seconds - NTP_UNIX_OFFSET_SECONDS;
    let fraction_nanos = ((timestamp.fraction as u128 * NANOS_PER_SECOND as u128) >> 32) as i128;
    unix_seconds as i128 * NANOS_PER_SECOND + fraction_nanos
}

/// Build the 48-byte client request in `buffer`: leap indicator 0, version 4,
/// mode 3 (client), zero stratum, poll and precision, and `transmit` in the
/// transmit timestamp field. Everything else stays zero.
pub fn build_request(transmit: NtpTimestamp, buffer: &mut [u8; PACKET_LENGTH]) {
    buffer.fill(0);
    buffer[0] = header(0, VERSION, MODE_CLIENT);
    write_timestamp(&mut buffer[40..48], transmit);
}

/// Why a server answer was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    /// Fewer than 48 bytes were received.
    TooShort,
    /// The mode is not 4 (server).
    WrongMode,
    /// The version is not 4.
    WrongVersion,
    /// Leap indicator 3: the server clock is unsynchronized.
    LeapAlarm,
    /// Stratum outside 1 to 15: 0 is a kiss-of-death code, 16 and above mean an
    /// unsynchronized server.
    StratumOutOfRange,
    /// The transmit timestamp is zero.
    ZeroTransmit,
    /// The originate timestamp is not the one this client sent, so the answer
    /// does not belong to this exchange.
    OriginateMismatch,
    /// The measured round trip exceeds the accepted bound.
    RoundTripTooLong,
}

/// A validated server answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Response {
    pub stratum: u8,
    pub leap_indicator: u8,
    pub originate: NtpTimestamp,
    pub receive: NtpTimestamp,
    pub transmit: NtpTimestamp,
    /// The transmit timestamp as Unix nanoseconds, with the era resolved
    /// against the client's estimate.
    pub transmit_unix_nanos: i128,
}

/// Parse and validate a server answer (RFC 4330 section 4): at least 48 bytes,
/// mode 4, version 4, leap indicator not 3, stratum 1 to 15, non-zero transmit
/// timestamp, and an originate timestamp equal to the transmit timestamp this
/// client sent.
pub fn parse_response(
    packet: &[u8],
    expected_originate: NtpTimestamp,
    estimate_unix_nanos: i128,
) -> Result<Response, ValidationError> {
    if packet.len() < PACKET_LENGTH {
        return Err(ValidationError::TooShort);
    }
    let first = packet[0];
    let leap_indicator = first >> 6;
    let version = (first >> 3) & 0b111;
    let mode = first & 0b111;
    if mode != MODE_SERVER {
        return Err(ValidationError::WrongMode);
    }
    if version != VERSION {
        return Err(ValidationError::WrongVersion);
    }
    if leap_indicator == LEAP_INDICATOR_ALARM {
        return Err(ValidationError::LeapAlarm);
    }
    let stratum = packet[1];
    if !(STRATUM_MINIMUM..=STRATUM_MAXIMUM).contains(&stratum) {
        return Err(ValidationError::StratumOutOfRange);
    }
    let originate = read_timestamp(&packet[24..32]);
    let receive = read_timestamp(&packet[32..40]);
    let transmit = read_timestamp(&packet[40..48]);
    if transmit.is_zero() {
        return Err(ValidationError::ZeroTransmit);
    }
    if originate != expected_originate {
        return Err(ValidationError::OriginateMismatch);
    }
    Ok(Response {
        stratum,
        leap_indicator,
        originate,
        receive,
        transmit,
        transmit_unix_nanos: unix_nanos_from_ntp(transmit, estimate_unix_nanos),
    })
}

/// The timing of one exchange, measured with the monotonic clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExchangeTiming {
    /// The validated server transmit timestamp, as Unix nanoseconds.
    pub server_transmit_unix_nanos: i128,
    /// Monotonic instant just before the request was sent.
    pub sent_monotonic: Duration,
    /// Monotonic instant just after the answer was received.
    pub received_monotonic: Duration,
}

impl ExchangeTiming {
    /// The round trip measured with the monotonic clock.
    pub fn round_trip(&self) -> Duration {
        self.received_monotonic - self.sent_monotonic
    }

    /// UTC at the receive instant: the server transmit time plus half the
    /// round trip. A round trip above `maximum_round_trip` is rejected, because
    /// the answer is then too old or too forged to trust.
    pub fn utc_at_receive_nanos(
        &self,
        maximum_round_trip: Duration,
    ) -> Result<i128, ValidationError> {
        let round_trip = self.round_trip();
        if round_trip > maximum_round_trip {
            return Err(ValidationError::RoundTripTooLong);
        }
        Ok(self.server_transmit_unix_nanos + round_trip.as_nanos() as i128 / 2)
    }
}

/// Read the 8-byte big-endian NTP timestamp at the start of `bytes`.
pub fn read_timestamp(bytes: &[u8]) -> NtpTimestamp {
    NtpTimestamp {
        seconds: u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        fraction: u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
    }
}

/// Write `timestamp` as 8 big-endian bytes at the start of `bytes`.
pub fn write_timestamp(bytes: &mut [u8], timestamp: NtpTimestamp) {
    bytes[..4].copy_from_slice(&timestamp.seconds.to_be_bytes());
    bytes[4..].copy_from_slice(&timestamp.fraction.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-01-01 00:00:00 UTC, in Unix nanoseconds.
    const UNIX_2026: i128 = 1_767_225_600_000_000_000;

    fn response_packet(mutate: impl FnOnce(&mut [u8; PACKET_LENGTH])) -> [u8; PACKET_LENGTH] {
        let mut packet = [0u8; PACKET_LENGTH];
        packet[0] = header(0, VERSION, MODE_SERVER);
        packet[1] = 2;
        write_timestamp(&mut packet[24..32], NtpTimestamp::ZERO);
        write_timestamp(&mut packet[32..40], NtpTimestamp::ZERO);
        write_timestamp(
            &mut packet[40..48],
            ntp_from_unix_nanos(UNIX_2026 + 500_000_000),
        );
        mutate(&mut packet);
        packet
    }

    #[test]
    fn type_sizes_stay_small() {
        assert_eq!(core::mem::size_of::<NtpTimestamp>(), 8);
        assert_eq!(core::mem::size_of::<ExchangeTiming>(), 48);
        assert_eq!(core::mem::size_of::<ValidationError>(), 1);
    }

    #[test]
    fn request_has_version_4_mode_3_and_only_the_transmit_field() {
        let transmit = ntp_from_unix_nanos(UNIX_2026);
        let mut buffer = [0xAAu8; PACKET_LENGTH];
        build_request(transmit, &mut buffer);
        assert_eq!(buffer[0], 0x23, "leap 0, version 4, mode 3");
        assert_eq!(&buffer[1..40], &[0u8; 39]);
        assert_eq!(read_timestamp(&buffer[40..48]), transmit);
    }

    #[test]
    fn ntp_and_unix_nanos_round_trip() {
        for unix_nanos in [
            0,
            1,
            -1,
            UNIX_2026,
            UNIX_2026 + 123_456_789,
            -(NTP_UNIX_OFFSET_SECONDS as i128) * NANOS_PER_SECOND, // 1900-01-01
        ] {
            let timestamp = ntp_from_unix_nanos(unix_nanos);
            // The fraction has a resolution of about 0.23 ns, so the round
            // trip can lose one nanosecond.
            assert!(
                (unix_nanos_from_ntp(timestamp, unix_nanos) - unix_nanos).abs() <= 1,
                "round trip of {unix_nanos}"
            );
        }
        // Whole seconds round trip exactly.
        assert_eq!(
            unix_nanos_from_ntp(ntp_from_unix_nanos(NANOS_PER_SECOND), NANOS_PER_SECOND),
            NANOS_PER_SECOND
        );
    }

    #[test]
    fn fraction_encoding_is_big_endian_and_exact_at_powers_of_two() {
        let timestamp = ntp_from_unix_nanos(NANOS_PER_SECOND / 2);
        assert_eq!(timestamp.fraction, 1 << 31);
        let timestamp = ntp_from_unix_nanos(NANOS_PER_SECOND / 4);
        assert_eq!(timestamp.fraction, 1 << 30);
        assert_eq!(unix_nanos_from_ntp(timestamp, 0), NANOS_PER_SECOND / 4);
    }

    #[test]
    fn era_zero_decodes_against_a_close_estimate() {
        // 3_976_000_000 NTP seconds is 2025-12-05, inside era 0 (1900 to 2036).
        let timestamp = NtpTimestamp {
            seconds: 3_976_000_000,
            fraction: 0,
        };
        let estimate = 1_767_225_600_000_000_000; // 2026-01-01
        assert_eq!(
            unix_nanos_from_ntp(timestamp, estimate),
            (3_976_000_000i128 - NTP_UNIX_OFFSET_SECONDS as i128) * NANOS_PER_SECOND
        );
    }

    #[test]
    fn era_one_resolves_after_the_2036_rollover() {
        // Seconds 100 in era 1 is 2036-02-07 plus 100 seconds, Unix 2_085_978_596.
        let timestamp = NtpTimestamp {
            seconds: 100,
            fraction: 0,
        };
        let unix_2036 = 2_085_978_596i128 * NANOS_PER_SECOND;
        assert_eq!(unix_nanos_from_ntp(timestamp, unix_2036), unix_2036);
        // An estimate in era 0 still resolves to era 1, because 2036 is closer
        // to 2026 than 1900 is.
        assert_eq!(unix_nanos_from_ntp(timestamp, UNIX_2026), unix_2036);
    }

    #[test]
    fn era_minus_one_resolves_before_1900_in_principle() {
        // Seconds 2^32 - 1 in era -1 is one second before 1900, Unix -2_208_988_801.
        let timestamp = NtpTimestamp {
            seconds: u32::MAX,
            fraction: 0,
        };
        let estimate = -(NTP_UNIX_OFFSET_SECONDS as i128 + 1) * NANOS_PER_SECOND;
        assert_eq!(unix_nanos_from_ntp(timestamp, estimate), estimate);
    }

    #[test]
    fn valid_response_passes_and_keeps_its_fields() {
        let sent = ntp_from_unix_nanos(UNIX_2026);
        let packet = response_packet(|packet| write_timestamp(&mut packet[24..32], sent));
        let response = parse_response(&packet, sent, UNIX_2026).unwrap();
        assert_eq!(response.stratum, 2);
        assert_eq!(response.leap_indicator, 0);
        assert_eq!(response.originate, sent);
        assert_eq!(
            response.transmit_unix_nanos,
            UNIX_2026 + NANOS_PER_SECOND / 2
        );
        // A longer packet (extension fields) is still accepted.
        let mut longer = packet.to_vec();
        longer.extend_from_slice(&[0u8; 20]);
        assert!(parse_response(&longer, sent, UNIX_2026).is_ok());
    }

    #[test]
    fn every_validation_rule_rejects() {
        let sent = ntp_from_unix_nanos(UNIX_2026);
        let estimate = UNIX_2026;

        assert_eq!(
            parse_response(&[0u8; PACKET_LENGTH - 1], sent, estimate),
            Err(ValidationError::TooShort)
        );

        type Mutator = fn(&mut [u8; PACKET_LENGTH]);
        let cases: [(&str, Mutator, ValidationError); 7] = [
            (
                "mode 5 broadcast",
                |p| p[0] = header(0, VERSION, 5),
                ValidationError::WrongMode,
            ),
            (
                "mode 1 symmetric",
                |p| p[0] = header(0, VERSION, 1),
                ValidationError::WrongMode,
            ),
            (
                "version 3",
                |p| p[0] = header(0, 3, MODE_SERVER),
                ValidationError::WrongVersion,
            ),
            (
                "leap alarm",
                |p| p[0] = header(3, VERSION, MODE_SERVER),
                ValidationError::LeapAlarm,
            ),
            (
                "stratum 0 kiss of death",
                |p| p[1] = 0,
                ValidationError::StratumOutOfRange,
            ),
            (
                "stratum 16",
                |p| p[1] = 16,
                ValidationError::StratumOutOfRange,
            ),
            (
                "zero transmit",
                |p| p[40..48].fill(0),
                ValidationError::ZeroTransmit,
            ),
        ];
        for (name, mutate, expected) in cases {
            let packet = response_packet(mutate);
            assert_eq!(
                parse_response(&packet, sent, estimate),
                Err(expected),
                "{name}"
            );
        }

        // A different originate timestamp is someone else's answer.
        let packet = response_packet(|_| {});
        assert_eq!(
            parse_response(&packet, ntp_from_unix_nanos(UNIX_2026 + 1), estimate),
            Err(ValidationError::OriginateMismatch)
        );
    }

    #[test]
    fn stratum_boundaries_are_accepted() {
        let sent = ntp_from_unix_nanos(UNIX_2026);
        for stratum in [STRATUM_MINIMUM, STRATUM_MAXIMUM] {
            let packet = response_packet(|p| {
                p[1] = stratum;
                write_timestamp(&mut p[24..32], sent);
            });
            assert!(
                parse_response(&packet, sent, UNIX_2026).is_ok(),
                "stratum {stratum}"
            );
        }
    }

    #[test]
    fn utc_at_receive_adds_half_the_round_trip() {
        let timing = ExchangeTiming {
            server_transmit_unix_nanos: UNIX_2026,
            sent_monotonic: Duration::from_millis(1_000),
            received_monotonic: Duration::from_millis(1_040),
        };
        assert_eq!(timing.round_trip(), Duration::from_millis(40));
        assert_eq!(
            timing
                .utc_at_receive_nanos(Duration::from_secs(10))
                .unwrap(),
            UNIX_2026 + 20_000_000
        );
    }

    #[test]
    fn an_overly_long_round_trip_is_rejected() {
        let timing = ExchangeTiming {
            server_transmit_unix_nanos: UNIX_2026,
            sent_monotonic: Duration::ZERO,
            received_monotonic: Duration::from_secs(11),
        };
        assert_eq!(
            timing.utc_at_receive_nanos(Duration::from_secs(10)),
            Err(ValidationError::RoundTripTooLong)
        );
        assert!(timing.utc_at_receive_nanos(Duration::from_secs(11)).is_ok());
    }
}
