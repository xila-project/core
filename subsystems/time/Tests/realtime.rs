use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use time::{
    ClockSource, Error, Manager, MonotonicClock, RealtimeSample, RealtimeSource, Timestamp,
};

struct Clock(AtomicU64);
impl MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::Relaxed))
    }
}
fn sample(nanos: i128) -> RealtimeSample {
    RealtimeSample {
        timestamp: Timestamp::from_nanos(nanos),
        source: ClockSource::Manual,
        resolution: Duration::from_millis(1),
        accuracy: None,
    }
}
#[test]
fn boot_is_invalid_until_explicitly_synchronized() {
    let clock = Clock(AtomicU64::new(500));
    let manager = Manager::with_clock(&clock);
    assert_eq!(manager.status().correction_nanoseconds, 0);
    assert_eq!(manager.status().synchronization, None);
    assert_eq!(manager.realtime_now(), Err(Error::Unsynchronized));
    clock.0.store(1500, Ordering::Relaxed);
    assert_eq!(manager.uptime(), Duration::from_secs(1));
    assert_eq!(manager.realtime_now(), Err(Error::Unsynchronized));
    manager.set_realtime(sample(10_000_000_001)).unwrap();
    assert_eq!(manager.realtime_now().unwrap().as_nanos(), 10_000_000_001);
    clock.0.store(2500, Ordering::Relaxed);
    assert_eq!(manager.realtime_now().unwrap().as_nanos(), 11_000_000_001);
    let sync = manager.status().synchronization.unwrap();
    assert_eq!(sync.source, ClockSource::Manual);
    assert_eq!(sync.at_monotonic, Duration::from_millis(1500));
    assert_eq!(sync.accuracy, None);
    assert_eq!(manager.realtime_resolution(), Duration::from_millis(1));
}
#[test]
fn repeated_signed_steps_preserve_monotonic_deadlines() {
    let clock = Clock(AtomicU64::new(5000));
    let manager = Manager::with_clock(&clock);
    let deadline = manager.monotonic_now() + Duration::from_secs(10);
    for nanos in [100_000_000_000, 1_000_000_000, -1, -2_000_000_001, 0] {
        manager.set_realtime(sample(nanos)).unwrap();
        assert_eq!(manager.realtime_now().unwrap().as_nanos(), nanos);
        assert_eq!(manager.monotonic_now(), Duration::from_secs(5));
        assert_eq!(manager.uptime(), Duration::ZERO);
        assert!(manager.monotonic_now() < deadline);
    }
    assert_eq!(manager.status().correction_nanoseconds, -5_000_000_000);
    clock.0.store(15000, Ordering::Relaxed);
    assert_eq!(manager.monotonic_now(), deadline);
    assert_eq!(manager.realtime_now().unwrap().as_nanos(), 10_000_000_000);
}
#[test]
fn overflow_is_reported_without_replacing_previous_synchronization() {
    let clock = Clock(AtomicU64::new(1));
    let manager = Manager::with_clock(&clock);
    manager.set_realtime(sample(42)).unwrap();
    let before = manager.status();
    assert_eq!(
        manager.set_realtime(sample(i128::MIN)),
        Err(Error::Overflow)
    );
    assert_eq!(manager.status(), before);
    assert_eq!(manager.realtime_now().unwrap().as_nanos(), 42);
    manager.set_realtime(sample(i128::MAX)).unwrap();
    clock.0.store(2, Ordering::Relaxed);
    assert_eq!(manager.realtime_now(), Err(Error::Overflow));
    manager.set_realtime(sample(0)).unwrap();
    assert_eq!(manager.realtime_now().unwrap().as_nanos(), 0);
}
#[test]
fn unsigned_and_display_conversions_are_checked() {
    assert_eq!(
        Timestamp::from_nanos(-1).try_into_duration(),
        Err(Error::Overflow)
    );
    assert_eq!(Timestamp::from_nanos(-1).try_as_seconds(), Ok(-1));
    assert_eq!(
        Timestamp::from_nanos(-1_000_000_001).try_as_seconds(),
        Ok(-2)
    );
    assert_eq!(
        Timestamp::from_nanos(i128::MAX).try_into_duration(),
        Err(Error::Overflow)
    );
    assert_eq!(
        Timestamp::from_nanos(i128::MAX).try_as_seconds(),
        Err(Error::Overflow)
    );
    assert_eq!(
        Timestamp::from_nanos(1_000_000_001).try_into_duration(),
        Ok(Duration::new(1, 1))
    );
    assert_eq!(
        Timestamp::from(Duration::MAX).try_into_duration(),
        Ok(Duration::MAX)
    );
}
struct Source;
impl RealtimeSource for Source {
    fn read(&self) -> time::Result<RealtimeSample> {
        Ok(RealtimeSample {
            source: ClockSource::Rtc,
            accuracy: Some(Duration::from_secs(2)),
            ..sample(-100)
        })
    }
}
struct Failed;
impl RealtimeSource for Failed {
    fn read(&self) -> time::Result<RealtimeSample> {
        Err(Error::DeviceError(file_system::Error::InternalError))
    }
}
#[test]
fn explicit_source_updates_are_atomic_and_not_polled_by_reads() {
    let clock = Clock(AtomicU64::new(1));
    let manager = Manager::with_monotonic_clock(&Source, &clock).unwrap();
    assert_eq!(manager.realtime_now().unwrap().as_nanos(), -100);
    let status = manager.status();
    assert_eq!(status.synchronization.unwrap().source, ClockSource::Rtc);
    assert_eq!(
        status.synchronization.unwrap().accuracy,
        Some(Duration::from_secs(2))
    );
    assert!(manager.synchronize(&Failed).is_err());
    assert_eq!(manager.status(), status);
    manager.invalidate();
    assert_eq!(manager.realtime_now(), Err(Error::Unsynchronized));
    assert_eq!(manager.status().correction_nanoseconds, 0);
    assert_eq!(manager.status().synchronization, None);
    assert_eq!(
        Manager::with_clock(&clock).status().correction_nanoseconds,
        0
    );
}

#[test]
fn invalid_sample_keeps_clock_state_and_custom_resolution_is_respected() {
    struct Coarse;
    impl MonotonicClock for Coarse {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
        fn resolution(&self) -> Duration {
            Duration::from_secs(1)
        }
    }
    let manager = Manager::with_clock(&Coarse);
    assert_eq!(manager.realtime_resolution(), Duration::from_secs(1));
    let mut update = sample(123);
    update.resolution = Duration::ZERO;
    assert_eq!(manager.set_realtime(update), Err(Error::InvalidResolution));
    assert_eq!(manager.realtime_now(), Err(Error::Unsynchronized));
    manager.set_realtime(sample(123)).unwrap();
    assert_eq!(manager.realtime_resolution(), Duration::from_secs(1));
    let before = manager.status();
    assert_eq!(manager.set_realtime(update), Err(Error::InvalidResolution));
    assert_eq!(manager.status(), before);
    assert!(Manager::with_monotonic_clock(&Failed, &Coarse).is_err());
}

#[test]
fn concurrent_readers_observe_complete_corrections() {
    let clock = Clock(AtomicU64::new(1));
    let manager = Manager::with_clock(&clock);
    manager.set_realtime(sample(-1)).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for _ in 0..1000 {
                manager.set_realtime(sample(i128::MAX)).unwrap();
                manager.set_realtime(sample(-1)).unwrap();
            }
        });
        for _ in 0..2 {
            scope.spawn(|| {
                for _ in 0..1000 {
                    let value = manager.realtime_now().unwrap().as_nanos();
                    assert!(value == -1 || value == i128::MAX);
                }
            });
        }
    });
}
