//! Real-time clock integration with the system clock (feature `services`).
//!
//! This module is runtime policy around the kernel clock, not part of it: the
//! `Manager`'s critical section never does I/O, and an external RTC chip on an
//! I2C bus blocks while it is read or written. Both operations therefore run
//! outside the manager, in the runtime that assembles it:
//!
//! - At start-up, [`synchronize_from_real_time_clock`] performs the "hardware
//!   clock to system" step (like Linux `hctosys`): when the clock holds a valid
//!   time that is not earlier than the platform's minimum timestamp (the build
//!   date), UTC is set from it with [`ClockSource::Rtc`]; otherwise the current
//!   fallback (the build date) stays.
//! - [`write_back_to_real_time_clock`] is the "system to hardware clock" step
//!   (like Linux `systohc`): a task that writes the RTC when UTC is known to be
//!   good, and about every hour while the latest synchronization is from the
//!   network or set manually. It never writes back a time whose source is the
//!   RTC itself or the build-date fallback, so a wrong RTC never propagates.

use core::time::Duration;

use peripherals::real_time_clock::{Error as ControllerError, RealTimeClockController, Time};

use crate::{ClockSource, Manager, RealtimeSample, Timestamp};

/// How often the write-back task rewrites the RTC while UTC stays synchronized
/// from the network or manually.
pub const WRITE_BACK_INTERVAL: Duration = Duration::from_secs(3_600);

/// Error of [`synchronize_from_real_time_clock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynchronizeError {
    /// The clock controller failed.
    Controller(ControllerError),
    /// The manager rejected the sample (overflow or zero resolution).
    Manager(crate::Error),
}

impl core::fmt::Display for SynchronizeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Controller(error) => write!(formatter, "real-time clock read failed: {error}"),
            Self::Manager(error) => {
                write!(formatter, "time manager rejected the sample: {error:?}")
            }
        }
    }
}

/// Converts a controller time to a kernel timestamp.
fn timestamp_from_time(time: Time) -> Timestamp {
    Timestamp::from_nanos(i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanoseconds))
}

/// Converts a kernel timestamp to a controller time, or `None` when it does not
/// fit signed Unix seconds.
fn time_from_timestamp(timestamp: Timestamp) -> Option<Time> {
    let seconds = timestamp.try_as_seconds().ok()?;
    let nanoseconds = timestamp.as_nanos() - i128::from(seconds) * 1_000_000_000;
    // `try_as_seconds` rounds toward the past, so this is in `0..1_000_000_000`.
    Some(Time::new(seconds, nanoseconds as u32))
}

/// Start-up read, "hardware clock to system" (like Linux `hctosys`).
///
/// Reads `controller` once. When it holds a valid time that is not earlier than
/// `minimum` (the platform's minimum timestamp, the build date), UTC is set from
/// it with [`ClockSource::Rtc`] and `Ok(true)` is returned. When the clock holds
/// no valid time, reads earlier than `minimum`, or cannot be read, the current
/// UTC (the build-date fallback) is kept and `Ok(false)` is returned; a
/// controller read error is reported as `Err`.
pub fn synchronize_from_real_time_clock(
    manager: &Manager<'_>,
    controller: &dyn RealTimeClockController,
    minimum: Timestamp,
) -> Result<bool, SynchronizeError> {
    let Some(time) = controller.read().map_err(SynchronizeError::Controller)? else {
        return Ok(false);
    };
    let timestamp = timestamp_from_time(time);
    if timestamp < minimum {
        return Ok(false);
    }
    manager
        .set_realtime(RealtimeSample {
            timestamp,
            source: ClockSource::Rtc,
            resolution: controller.resolution(),
            accuracy: None,
        })
        .map_err(SynchronizeError::Manager)?;
    Ok(true)
}

/// The current UTC as a controller time, but only when its source is the
/// network or a manual setting. The RTC itself, the host clock and the
/// build-date fallback are never written back to the clock.
fn writable_time(manager: &Manager<'_>) -> Option<Time> {
    // One lock section: `status` carries both the correction and the source.
    let status = manager.status();
    match status.synchronization?.source {
        ClockSource::Network | ClockSource::Manual => {}
        ClockSource::Rtc | ClockSource::BuildDate | ClockSource::Host => return None,
    }
    let now = (manager.monotonic_now().as_nanos() as i128) + status.correction_nanoseconds;
    time_from_timestamp(Timestamp::from_nanos(now))
}

/// One write-back evaluation: writes the current UTC to the RTC when its source
/// is network or manual. A failed write is logged once until a write succeeds
/// again.
fn write_back_once(
    manager: &Manager<'_>,
    controller: &dyn RealTimeClockController,
    failure_logged: &mut bool,
) {
    if let Some(time) = writable_time(manager) {
        match controller.write(time) {
            Ok(()) => *failure_logged = false,
            Err(error) => {
                if !*failure_logged {
                    log::error!("Real-time clock write-back failed: {error}");
                    *failure_logged = true;
                }
            }
        }
    }
}

/// Write-back task, "system to hardware clock" (like Linux `systohc`).
///
/// The runtime spawns this as its own task with the shared RTC controller (the
/// same lower half the device node uses). On every UTC change, and about every
/// [`WRITE_BACK_INTERVAL`] while UTC stays synchronized from the network or
/// manually, it writes the current UTC to the clock. It never writes while the
/// source is the RTC itself, the host clock or the build-date fallback.
pub async fn write_back_to_real_time_clock(
    manager: &Manager<'_>,
    controller: &dyn RealTimeClockController,
) -> ! {
    write_back_to_real_time_clock_with_interval(manager, controller, WRITE_BACK_INTERVAL).await
}

/// [`write_back_to_real_time_clock`] with a custom rewrite interval, for tests
/// and runtimes that want a different period than [`WRITE_BACK_INTERVAL`].
pub async fn write_back_to_real_time_clock_with_interval(
    manager: &Manager<'_>,
    controller: &dyn RealTimeClockController,
    interval: Duration,
) -> ! {
    let mut generation = manager.change_generation();
    let mut failure_logged = false;
    loop {
        write_back_once(manager, controller, &mut failure_logged);
        let changed = manager.wait_for_change(generation);
        let timer = embassy_time::Timer::after(embassy_time::Duration::from_micros(
            u64::try_from(interval.as_micros()).unwrap_or(u64::MAX),
        ));
        generation = match embassy_futures::select::select(changed, timer).await {
            embassy_futures::select::Either::First(generation) => generation,
            embassy_futures::select::Either::Second(()) => generation,
        };
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    reason = "host-only unit test uses core atomics for the fake monotonic clock"
)]
mod tests {
    use super::*;
    use crate::MonotonicClock;
    use alloc::boxed::Box;
    use core::sync::atomic::{AtomicU64, Ordering};
    use peripherals::real_time_clock::MockRealTimeClock;

    struct Clock(AtomicU64);
    impl MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::Relaxed))
        }
    }

    fn manager_at(millis: u64) -> Manager<'static> {
        let clock: &'static Clock = Box::leak(Box::new(Clock(AtomicU64::new(millis))));
        Manager::with_clock(clock)
    }

    fn sample(source: ClockSource, seconds: i64) -> RealtimeSample {
        RealtimeSample {
            timestamp: Timestamp::from_nanos(i128::from(seconds) * 1_000_000_000),
            source,
            resolution: Duration::from_secs(1),
            accuracy: None,
        }
    }

    const BUILD_DATE: i64 = 1_700_000_000;

    #[test]
    fn start_up_read_applies_a_valid_time_not_earlier_than_the_floor() {
        let manager = manager_at(0);
        let rtc = MockRealTimeClock::new(Some(Time::new(BUILD_DATE + 3_600, 5)));
        assert_eq!(
            synchronize_from_real_time_clock(
                &manager,
                &rtc,
                Timestamp::from_nanos(i128::from(BUILD_DATE) * 1_000_000_000)
            ),
            Ok(true)
        );
        assert_eq!(manager.synchronization().unwrap().source, ClockSource::Rtc);
        assert_eq!(
            manager.realtime_now().unwrap().as_nanos(),
            (i128::from(BUILD_DATE + 3_600)) * 1_000_000_000 + 5
        );
    }

    #[test]
    fn start_up_read_keeps_the_fallback_when_the_clock_is_invalid_or_early() {
        let manager = manager_at(0);
        // No valid time at all.
        let rtc = MockRealTimeClock::new(None);
        assert_eq!(
            synchronize_from_real_time_clock(
                &manager,
                &rtc,
                Timestamp::from_nanos(i128::from(BUILD_DATE) * 1_000_000_000)
            ),
            Ok(false)
        );
        assert_eq!(manager.synchronization(), None);
        // Earlier than the build-date floor.
        let rtc = MockRealTimeClock::new(Some(Time::new(BUILD_DATE - 1, 0)));
        assert_eq!(
            synchronize_from_real_time_clock(
                &manager,
                &rtc,
                Timestamp::from_nanos(i128::from(BUILD_DATE) * 1_000_000_000)
            ),
            Ok(false)
        );
        assert_eq!(manager.synchronization(), None);
        // Exactly the floor is accepted ("not earlier than").
        let rtc = MockRealTimeClock::new(Some(Time::new(BUILD_DATE, 0)));
        assert_eq!(
            synchronize_from_real_time_clock(
                &manager,
                &rtc,
                Timestamp::from_nanos(i128::from(BUILD_DATE) * 1_000_000_000)
            ),
            Ok(true)
        );
    }

    #[test]
    fn start_up_read_reports_a_controller_error() {
        let manager = manager_at(0);
        let rtc = MockRealTimeClock::new(None);
        rtc.fail_reads(true);
        assert_eq!(
            synchronize_from_real_time_clock(
                &manager,
                &rtc,
                Timestamp::from_nanos(i128::from(BUILD_DATE) * 1_000_000_000)
            ),
            Err(SynchronizeError::Controller(ControllerError::Other))
        );
        assert_eq!(manager.synchronization(), None);
    }

    #[test]
    fn write_back_only_happens_for_network_and_manual_sources() {
        let manager = manager_at(0);
        let rtc = MockRealTimeClock::new(None);
        let mut failure_logged = false;

        // Unsynchronized: no write.
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 0);

        // The build-date fallback is never written back.
        manager
            .set_realtime(sample(ClockSource::BuildDate, BUILD_DATE))
            .unwrap();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 0);

        // The RTC itself is never written back.
        manager
            .set_realtime(sample(ClockSource::Rtc, BUILD_DATE + 10))
            .unwrap();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 0);

        // The host clock is not a write-back source either.
        manager
            .set_realtime(sample(ClockSource::Host, BUILD_DATE + 20))
            .unwrap();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 0);

        // A manual setting is written back immediately.
        manager
            .set_realtime(sample(ClockSource::Manual, BUILD_DATE + 30))
            .unwrap();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 1);
        assert_eq!(rtc.time(), Some(Time::new(BUILD_DATE + 30, 0)));

        // A network synchronization is written back too.
        manager
            .set_realtime(sample(ClockSource::Network, BUILD_DATE + 40))
            .unwrap();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 2);
        assert_eq!(rtc.time(), Some(Time::new(BUILD_DATE + 40, 0)));

        // Periodic: while the source stays network or manual, every evaluation
        // writes again.
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 3);

        // Invalidation stops the write-back.
        manager.invalidate();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 3);
    }

    #[test]
    fn write_back_preserves_subsecond_nanoseconds() {
        let manager = manager_at(0);
        let rtc = MockRealTimeClock::new(None);
        let mut failure_logged = false;
        manager
            .set_realtime(RealtimeSample {
                timestamp: Timestamp::from_nanos(
                    i128::from(BUILD_DATE) * 1_000_000_000 + 123_456_789,
                ),
                source: ClockSource::Manual,
                resolution: Duration::from_nanos(1),
                accuracy: None,
            })
            .unwrap();
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.time(), Some(Time::new(BUILD_DATE, 123_456_789)));
    }

    #[test]
    fn a_failed_write_is_logged_once_until_one_succeeds() {
        use core::sync::atomic::AtomicUsize;

        struct CountingLogger;
        static ERRORS: AtomicUsize = AtomicUsize::new(0);
        impl log::LoggerTrait for CountingLogger {
            fn enabled(&self, _level: log::Level) -> bool {
                true
            }
            fn write(&self, _arguments: core::fmt::Arguments) {}
            fn log(&self, record: &log::Record) {
                if record.level == log::Level::Error {
                    ERRORS.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        static LOGGER: CountingLogger = CountingLogger;
        if !log::is_initialized() {
            log::initialize(&LOGGER).unwrap();
        }
        ERRORS.store(0, Ordering::Relaxed);

        let manager = manager_at(0);
        let rtc = MockRealTimeClock::new(None);
        rtc.fail_writes(true);
        manager
            .set_realtime(sample(ClockSource::Manual, BUILD_DATE))
            .unwrap();

        let mut failure_logged = false;
        write_back_once(&manager, &rtc, &mut failure_logged);
        write_back_once(&manager, &rtc, &mut failure_logged);
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(ERRORS.load(Ordering::Relaxed), 1, "logged once");
        assert_eq!(rtc.write_count(), 0);

        // A successful write resets the flag: the next failure logs again.
        rtc.fail_writes(false);
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(rtc.write_count(), 1);
        rtc.fail_writes(true);
        write_back_once(&manager, &rtc, &mut failure_logged);
        assert_eq!(ERRORS.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn timestamp_conversions_round_trip() {
        for (seconds, nanoseconds) in [
            (0, 0),
            (-1, 999_999_999),
            (1_767_225_600, 123_456_789),
            (i64::MIN, 0),
        ] {
            let time = Time::new(seconds, nanoseconds);
            assert_eq!(time_from_timestamp(timestamp_from_time(time)), Some(time));
        }
        // Out of i64 seconds range cannot become a controller time.
        assert_eq!(time_from_timestamp(Timestamp::from_nanos(i128::MAX)), None);
        assert_eq!(time_from_timestamp(Timestamp::from_nanos(i128::MIN)), None);
    }
}
