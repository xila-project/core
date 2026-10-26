#![cfg(feature = "runtime-clock")]
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use timezone::{Error, Zone};

struct Monotonic(AtomicU64);
impl time::MonotonicClock for Monotonic {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::Relaxed))
    }
}
fn sample(nanos: i128) -> time::RealtimeSample {
    time::RealtimeSample {
        timestamp: time::Timestamp::from_nanos(nanos),
        source: time::ClockSource::Manual,
        resolution: Duration::from_secs(1),
        accuracy: None,
    }
}
#[test]
fn invalid_realtime_is_an_error_without_an_invented_local_epoch() {
    let monotonic = Monotonic(AtomicU64::new(100));
    let clock = time::Manager::with_clock(&monotonic);
    assert_eq!(
        Zone::UTC.now(&clock).unwrap_err(),
        Error::Clock(time::Error::Unsynchronized)
    );
    clock.set_realtime(sample(0)).unwrap();
    assert_eq!(Zone::UTC.now(&clock).unwrap().utc_nanoseconds(), 0);
    clock.invalidate();
    assert_eq!(
        Zone::UTC.now(&clock).unwrap_err(),
        Error::Clock(time::Error::Unsynchronized)
    );
}
#[test]
fn runtime_arithmetic_and_civil_range_errors_are_preserved() {
    let monotonic = Monotonic(AtomicU64::new(0));
    let clock = time::Manager::with_clock(&monotonic);
    clock.set_realtime(sample(i128::MAX)).unwrap();
    assert_eq!(Zone::UTC.now(&clock).unwrap_err(), Error::OutOfRange);
    monotonic.0.store(1, Ordering::Relaxed);
    assert_eq!(
        Zone::UTC.now(&clock).unwrap_err(),
        Error::Clock(time::Error::Overflow)
    );
}
#[cfg(feature = "zone-europe-paris")]
#[test]
fn local_zone_selection_never_changes_utc_or_monotonic_deadlines() {
    let monotonic = Monotonic(AtomicU64::new(100));
    let clock = time::Manager::with_clock(&monotonic);
    let zone = Zone::get("Europe/Paris").unwrap();
    let deadline = clock.monotonic_now() + Duration::from_secs(10);
    for (epoch, offset) in [(1705320000, 3600), (1721044800, 7200), (-1, 3600)] {
        clock.set_realtime(sample(epoch * 1_000_000_000)).unwrap();
        let before = clock.realtime_now().unwrap();
        let state = clock.status();
        let local = zone.now(&clock).unwrap();
        assert_eq!(local.utc_nanoseconds(), before.as_nanos());
        assert_eq!(local.offset_seconds(), offset);
        assert_eq!(clock.realtime_now().unwrap(), before);
        assert_eq!(clock.status(), state);
        assert_eq!(clock.monotonic_now(), Duration::from_secs(100));
        assert_eq!(deadline, Duration::from_secs(110));
    }
}
