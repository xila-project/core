use crate::{Error, Result};
use core::time::Duration;
use file_system::DirectCharacterDevice;

/// Nominal resolution of the default monotonic source, rounded up to nanoseconds.
/// Actual precision can be coarser (for example, browser privacy restrictions).
pub const MONOTONIC_RESOLUTION: Duration =
    Duration::from_nanos(1_000_000_000u64.div_ceil(embassy_time::TICK_HZ));

/// An elapsed-time source with a stable, arbitrary epoch.
///
/// Values must never decrease or wrap during a boot, including across concurrent
/// reads. UTC corrections must not affect this source. Suspend accounting is a
/// platform property; callers must not assume suspended time is included.
pub trait MonotonicClock: Send + Sync {
    fn now(&self) -> Duration;
}

/// The executor's Embassy clock, shared with task timers and WASI deadlines.
///
/// Native uses `std::time::Instant`; browsers use `performance.now()` through
/// Embassy's wasm driver. Both currently use 1 MHz ticks (1 microsecond nominal
/// resolution). Suspend behavior follows the OS/browser and is not portable;
/// browser throttling can delay wakeups independently of clock readings.
///
/// Embedded platforms must supply an Embassy hardware time driver and tick rate.
/// If its counter stops in low-power sleep, this clock excludes that interval.
/// Deep sleep/reset starts a new epoch; instants must never be persisted.
/// Embassy's u64 tick counter must not wrap within a boot (at 1 MHz its range is
/// about 584,000 years). Hardware drivers must extend narrower counters.
pub struct EmbassyClock;

impl MonotonicClock for EmbassyClock {
    fn now(&self) -> Duration {
        duration_from_ticks(embassy_time::Instant::now().as_ticks())
    }
}

static EMBASSY_CLOCK: EmbassyClock = EmbassyClock;

fn duration_from_ticks(ticks: u64) -> Duration {
    // Split before scaling: Instant::as_nanos() can overflow a u64 even though
    // the timestamp still fits in a core Duration.
    Duration::new(
        ticks / embassy_time::TICK_HZ,
        ((ticks % embassy_time::TICK_HZ) as u128 * 1_000_000_000 / embassy_time::TICK_HZ as u128)
            as u32,
    )
}

pub struct Manager<'a> {
    device: &'a (dyn DirectCharacterDevice + Send + Sync),
    monotonic_clock: &'a dyn MonotonicClock,
    start_time: Duration,
}

impl<'a> Manager<'a> {
    pub fn new(device: &'a (dyn DirectCharacterDevice + Send + Sync)) -> Result<Self> {
        Self::with_monotonic_clock(device, &EMBASSY_CLOCK)
    }

    /// Construct a manager with a platform-specific monotonic source.
    /// Its epoch must agree with the executor when used for absolute deadlines.
    pub fn with_monotonic_clock(
        device: &'a (dyn DirectCharacterDevice + Send + Sync),
        monotonic_clock: &'a dyn MonotonicClock,
    ) -> Result<Self> {
        Self::get_current_time_from_device(device)?;
        Ok(Self {
            device,
            monotonic_clock,
            start_time: monotonic_clock.now(),
        })
    }

    /// Monotonic time since the source's arbitrary epoch, independent of UTC.
    /// Use this epoch for absolute deadlines; use `uptime` for manager lifetime.
    pub fn monotonic_now(&self) -> Duration {
        self.monotonic_clock.now()
    }

    /// Elapsed time since this manager was initialized, independent of UTC.
    pub fn uptime(&self) -> Duration {
        self.monotonic_now() - self.start_time
    }

    /// Milliseconds modulo 2^32, for wrapping tick consumers such as LVGL.
    /// Wraps every ~49.7 days; compute differences with wrapping subtraction
    /// and sample at intervals shorter than one complete wrap.
    pub fn monotonic_milliseconds_wrapping(&self) -> u32 {
        self.monotonic_now().as_millis() as u32
    }

    /// UTC realtime since the Unix epoch, read from the platform device.
    /// This clock may jump in either direction; never use it for elapsed time.
    pub fn realtime_now(&self) -> Result<Duration> {
        Self::get_current_time_from_device(self.device)
    }

    fn get_current_time_from_device(device: &dyn DirectCharacterDevice) -> Result<Duration> {
        let mut current_time = Duration::default();
        let current_time_raw = unsafe {
            core::slice::from_raw_parts_mut(
                &mut current_time as *mut Duration as *mut u8,
                core::mem::size_of::<Duration>(),
            )
        };
        device
            .read_sync(current_time_raw, 0)
            .map_err(Error::DeviceError)?;
        Ok(current_time)
    }
}

pub fn initialize(
    driver: &'static (dyn DirectCharacterDevice + Send + Sync),
) -> Result<Manager<'static>> {
    Manager::new(driver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_conversion_does_not_overflow_at_counter_limit() {
        assert_eq!(
            duration_from_ticks(u64::MAX),
            Duration::new(18_446_744_073_709, 551_615_000)
        );
        assert_eq!(duration_from_ticks(0), Duration::ZERO);
        assert_eq!(
            duration_from_ticks(embassy_time::TICK_HZ),
            Duration::from_secs(1)
        );
    }
}
