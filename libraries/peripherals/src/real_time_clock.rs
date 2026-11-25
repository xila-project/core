//! Real-time clock lower-half interface.
//!
//! A real-time clock (RTC) stores wall-clock time (UTC) across resets and, on
//! chips with a battery-backed domain, across power-off. Like Linux, Xila keeps
//! RTC drivers separate from clocksource drivers: the RTC only supplies and
//! stores UTC, while timers, deadlines and uptime stay on the monotonic clock
//! of the `time` subsystem.
//!
//! A target implements [`RealTimeClockController`] once for its clock source:
//! the internal RTC of a microcontroller (for example the ESP32's always-on
//! registers through esp-hal's `rtc_cntl::Rtc`) or an external chip on an I2C
//! bus through the `rtcc` crate's `DateTimeAccess` (see
//! `drivers_peripherals::real_time_clock::ExternalRealTimeClock`). The upper
//! half (`drivers_peripherals::real_time_clock`) mounts the controller as
//! `/devices/time/real_time_clock` and adapts it to the
//! `device::real_time_clock` contract.
//!
//! Each method is a single attempt. An external chip on a blocking I2C bus may
//! take some time; that is why the time subsystem's write-back runs in its own
//! task and never inside the clock manager's critical section. `read` returns
//! `Ok(None)` when the clock holds no valid time (for example after power-off
//! on a chip without a battery-backed domain, or when the reading is earlier
//! than the platform's minimum timestamp, the build date).

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::MockRealTimeClock;

/// Nanoseconds in one second; the exclusive upper bound of [`Time::nanoseconds`].
pub const NANOSECONDS_PER_SECOND: u32 = 1_000_000_000;

/// A wall-clock time in UTC: signed Unix seconds since 1970-01-01T00:00:00Z
/// plus nanoseconds within the second, without timezone or DST.
///
/// 16 bytes on 32-bit targets where `i64` is 8-aligned (RISC-V, ARM) and on
/// 64-bit targets, 12 bytes where `i64` is 4-aligned (Xtensa).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Time {
    pub seconds: i64,
    /// Nanoseconds within the second, always below [`NANOSECONDS_PER_SECOND`].
    pub nanoseconds: u32,
}

impl Time {
    pub const fn new(seconds: i64, nanoseconds: u32) -> Self {
        assert!(
            nanoseconds < NANOSECONDS_PER_SECOND,
            "nanoseconds must be below one second"
        );
        Self {
            seconds,
            nanoseconds,
        }
    }
}

/// Error of a real-time clock controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The time is out of range for the clock (for example a date the chip
    /// cannot store).
    Invalid,
    /// The clock is busy with another operation.
    Busy,
    /// Any other hardware or driver failure, including a lost or unpowered clock.
    Other,
}

impl core::fmt::Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "real-time clock time is out of range",
            Self::Busy => "real-time clock is busy",
            Self::Other => "real-time clock operation failed",
        })
    }
}

impl core::error::Error for Error {}

/// Hardware-facing real-time clock controller.
///
/// Implementations are shared (`&'static` in practice) between the device node
/// and the time subsystem's write-back task, so the methods take `&self`, use
/// interior mutability and the trait requires `Send + Sync`.
pub trait RealTimeClockController: Send + Sync {
    /// Reads the time the clock holds. `Ok(None)` when the clock holds no
    /// valid time; an error when the clock cannot be read at all.
    fn read(&self) -> Result<Option<Time>, Error>;

    /// Sets the time the clock holds.
    fn write(&self, time: Time) -> Result<(), Error>;

    /// Nominal quantization of the clock's readings, reported as the
    /// resolution of the synchronization sample when the clock seeds UTC.
    fn resolution(&self) -> core::time::Duration {
        core::time::Duration::from_secs(1)
    }
}

/// One Julian century in seconds. A platform's maximum timestamp is its minimum
/// (the build date) plus this: a stored reading may not be later than the build
/// date by more than a century, so garbage in a chip's store registers after
/// power-on is never accepted.
pub const CENTURY_SECONDS: i64 = 3_155_760_000;

/// Converts a microsecond wall-clock reading (like the ESP32 RTC timer's
/// `current_time_us`) into Unix time.
///
/// Returns `None` when the reading is earlier than `minimum_seconds`. Platforms
/// pass the build date as the minimum: a clock that was never set, or that lost
/// power, reads as an early time and must not seed the system clock.
pub fn unix_time_from_microseconds(microseconds: u64, minimum_seconds: i64) -> Option<Time> {
    unix_time_from_microseconds_in_range(microseconds, minimum_seconds, i64::MAX)
}

/// Like [`unix_time_from_microseconds`], but also rejects readings later than
/// `maximum_seconds`, for example the build date plus [`CENTURY_SECONDS`].
pub fn unix_time_from_microseconds_in_range(
    microseconds: u64,
    minimum_seconds: i64,
    maximum_seconds: i64,
) -> Option<Time> {
    // u64 microseconds are at most about 584,000 years, which always fits in i64 seconds.
    let seconds = i64::try_from(microseconds / 1_000_000).ok()?;
    if !unix_seconds_in_range(seconds, minimum_seconds, maximum_seconds) {
        return None;
    }
    let nanoseconds = u32::try_from(microseconds % 1_000_000)
        .ok()?
        .checked_mul(1_000)?;
    Some(Time::new(seconds, nanoseconds))
}

/// Whether signed Unix seconds lie within `[minimum_seconds, maximum_seconds]`.
pub fn unix_seconds_in_range(seconds: i64, minimum_seconds: i64, maximum_seconds: i64) -> bool {
    (minimum_seconds..=maximum_seconds).contains(&seconds)
}

/// Converts Unix time into microseconds since the epoch, the inverse of
/// [`unix_time_from_microseconds`]. Returns `None` for a negative or
/// out-of-range time, which a microsecond counter cannot hold.
pub fn microseconds_from_unix_time(time: Time) -> Option<u64> {
    if time.nanoseconds >= NANOSECONDS_PER_SECOND {
        return None;
    }
    let microseconds = time
        .seconds
        .checked_mul(1_000_000)?
        .checked_add(i64::from(time.nanoseconds / 1_000))?;
    u64::try_from(microseconds).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_are_rejected_outside_the_floor_and_ceiling() {
        let minimum = 1_767_225_600;
        let maximum = minimum + CENTURY_SECONDS;
        let at_floor = u64::try_from(minimum).unwrap() * 1_000_000;
        let at_ceiling = u64::try_from(maximum).unwrap() * 1_000_000;
        // At the floor and at the ceiling the reading is valid.
        assert_eq!(
            unix_time_from_microseconds_in_range(at_floor, minimum, maximum),
            Some(Time::new(minimum, 0))
        );
        assert_eq!(
            unix_time_from_microseconds_in_range(at_ceiling, minimum, maximum),
            Some(Time::new(maximum, 0))
        );
        // Earlier than the floor (never set, lost power) and later than the
        // ceiling (garbage in the store registers after power-on): no time.
        assert_eq!(
            unix_time_from_microseconds_in_range(at_floor - 1_000_000, minimum, maximum),
            None
        );
        assert_eq!(
            unix_time_from_microseconds_in_range(at_ceiling + 1_000_000, minimum, maximum),
            None
        );
        assert!(unix_seconds_in_range(minimum, minimum, maximum));
        assert!(unix_seconds_in_range(maximum, minimum, maximum));
        assert!(!unix_seconds_in_range(minimum - 1, minimum, maximum));
        assert!(!unix_seconds_in_range(maximum + 1, minimum, maximum));
    }

    #[test]
    fn microseconds_convert_to_unix_time_and_back() {
        let time = unix_time_from_microseconds(1_767_225_600_123_456, 0);
        assert_eq!(time, Some(Time::new(1_767_225_600, 123_456_000)));
        assert_eq!(
            microseconds_from_unix_time(Time::new(1_767_225_600, 123_456_789)),
            Some(1_767_225_600_123_456)
        );
        assert_eq!(
            microseconds_from_unix_time(Time::new(0, 999_999_999)),
            Some(999_999)
        );
        // Sub-microsecond nanoseconds are truncated, not rounded.
        assert_eq!(microseconds_from_unix_time(Time::new(0, 999)), Some(0));
    }

    #[test]
    fn readings_earlier_than_the_minimum_are_not_valid() {
        // A clock that was never set reads near zero; the build date floors it.
        assert_eq!(unix_time_from_microseconds(0, 1_700_000_000), None);
        assert_eq!(
            unix_time_from_microseconds(1_699_999_999_999_999, 1_700_000_000),
            None
        );
        assert_eq!(
            unix_time_from_microseconds(1_700_000_000_000_000, 1_700_000_000),
            Some(Time::new(1_700_000_000, 0))
        );
        assert_eq!(
            unix_time_from_microseconds(u64::MAX, i64::MIN),
            Some(Time::new(
                i64::try_from(u64::MAX / 1_000_000).unwrap(),
                u32::try_from(u64::MAX % 1_000_000).unwrap() * 1_000
            ))
        );
    }

    #[test]
    fn out_of_range_times_do_not_convert_to_microseconds() {
        assert_eq!(microseconds_from_unix_time(Time::new(-1, 0)), None);
        assert_eq!(microseconds_from_unix_time(Time::new(i64::MAX, 0)), None);
        assert_eq!(
            microseconds_from_unix_time(Time {
                seconds: 0,
                nanoseconds: NANOSECONDS_PER_SECOND
            }),
            None
        );
    }
}
