#![no_std]

extern crate alloc;
use core::time::Duration;
use drivers_std::devices::TimeDevice;
use log::information;

#[test]
fn test_realtime_now() {
    let time_manager = time::Manager::new(&TimeDevice).unwrap();

    let current_time = time_manager.realtime_now().unwrap();

    information!("Current time : {current_time:?}");

    assert_ne!(current_time, Duration::default());
}

#[test]
fn test_uptime() {
    let time_manager = time::Manager::new(&TimeDevice).unwrap();

    let current_time = time_manager.uptime();

    information!("Time since startup : {current_time:?}");

    assert!(current_time < Duration::from_secs(1));
}

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};

struct WallClock {
    seconds: AtomicU64,
    failed: AtomicBool,
}

impl DirectBaseOperations for WallClock {
    fn read(
        &self,
        _: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            if self.failed.load(Ordering::Relaxed) {
                return Err(file_system::Error::InternalError);
            }
            let value = Duration::from_secs(self.seconds.load(Ordering::Relaxed));
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    (&value as *const Duration).cast::<u8>(),
                    size_of::<Duration>(),
                )
            };
            buffer[..bytes.len()].copy_from_slice(bytes);
            Ok(bytes.len())
        })
    }

    fn write(
        &self,
        _: &mut core::task::Context<'_>,
        _: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }
}
impl MountOperations for WallClock {}
impl DirectCharacterDevice for WallClock {}

struct Clock(AtomicU64);
impl time::MonotonicClock for Clock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::Relaxed))
    }
}

// Replacing monotonic time with UTC would change every elapsed observation here.
#[test]
fn utc_steps_do_not_change_uptime_ticks_or_deadlines() {
    let wall = WallClock {
        seconds: AtomicU64::new(1_000),
        failed: AtomicBool::new(false),
    };
    let clock = Clock(AtomicU64::new(500));
    let manager = time::Manager::with_monotonic_clock(&wall, &clock).unwrap();
    let deadline = manager.monotonic_now() + Duration::from_secs(30);
    assert_eq!(deadline, Duration::from_millis(30_500));
    assert_eq!(manager.uptime(), Duration::ZERO);

    for seconds in [2_000, 100] {
        wall.seconds.store(seconds, Ordering::Relaxed);
        assert_eq!(
            manager.realtime_now().unwrap(),
            Duration::from_secs(seconds)
        );
        assert_eq!(manager.monotonic_now(), Duration::from_millis(500));
        assert_eq!(manager.monotonic_milliseconds_wrapping(), 500);
        assert_eq!(manager.uptime(), Duration::ZERO);
        assert!(manager.monotonic_now() < deadline);
    }
    clock.0.store(30_499, Ordering::Relaxed);
    assert!(manager.monotonic_now() < deadline);
    clock.0.store(30_500, Ordering::Relaxed);
    assert_eq!(manager.monotonic_now(), deadline);
    assert_eq!(manager.uptime(), Duration::from_secs(30));
    let restarted = time::Manager::with_monotonic_clock(&wall, &clock).unwrap();
    assert_eq!(restarted.uptime(), Duration::ZERO);
    assert_eq!(restarted.monotonic_now(), deadline);

    wall.failed.store(true, Ordering::Relaxed);
    assert!(manager.realtime_now().is_err());
    assert_eq!(manager.uptime(), Duration::from_secs(30));
    assert_eq!(manager.monotonic_milliseconds_wrapping(), 30_500);
}

// LVGL computes tick differences with wrapping u32 subtraction.
#[test]
fn millisecond_tick_rollover_preserves_elapsed_time() {
    let clock = Clock(AtomicU64::new(4_294_967_290));
    let manager = time::Manager::with_monotonic_clock(&TimeDevice, &clock).unwrap();
    let before = manager.monotonic_milliseconds_wrapping();
    assert_eq!(before, 4_294_967_290);
    clock.0.store(4_294_967_305, Ordering::Relaxed);
    let after = manager.monotonic_milliseconds_wrapping();
    assert_eq!(after, 9);
    assert_eq!(after.wrapping_sub(before), 15);
    assert_eq!(manager.uptime(), Duration::from_millis(15));
}
