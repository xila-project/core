use crate::{
    Error, RealtimeSample, RealtimeSource, RealtimeStatus, Result, Synchronization, Timestamp,
};
use core::cell::RefCell;
use core::time::Duration;
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

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

    /// Nominal source quantization; override for custom platform clocks.
    fn resolution(&self) -> Duration {
        MONOTONIC_RESOLUTION
    }
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

/// System-owned UTC clock anchored to monotonic time, independent of shell lifetime.
///
/// `with_clock`/`unsynchronized` boot with zero correction and invalid UTC. Targets
/// without an RTC use these until manual or network initialization. `new` bootstraps
/// once from a trusted source (native/browser use the host clock); source failure is
/// returned so the runtime may explicitly fall back to an unsynchronized manager.
/// Nothing survives reset: persistence and source selection are runtime policy.
///
/// Corrections step immediately, in either direction; there is no slew. Monotonic
/// deadlines and uptime are unchanged. UTC subsequently advances at the monotonic
/// source's rate, including its suspend behavior; resynchronize after suspend when
/// that source stops during sleep. Nominal resolution is the coarser of the epoch
/// sample and monotonic clock. Accuracy is unknown unless supplied by the source,
/// and is not maintained as a bound as time elapses. No timezone or DST is applied.
///
/// State updates and reads are serialized with a short critical section, without
/// allocation or I/O while locked. Custom monotonic sources must be nonblocking
/// and safe to call in that critical section.
pub struct Manager<'a> {
    monotonic_clock: &'a dyn MonotonicClock,
    start_time: Duration,
    realtime: Mutex<CriticalSectionRawMutex, RefCell<RealtimeStatus>>,
}

impl<'a> Manager<'a> {
    pub fn new(source: &dyn RealtimeSource) -> Result<Self> {
        Self::with_monotonic_clock(source, &EMBASSY_CLOCK)
    }

    /// Construct a manager and initialize UTC once from a trusted epoch source.
    /// Its monotonic epoch must agree with the executor for absolute deadlines.
    pub fn with_monotonic_clock(
        source: &dyn RealtimeSource,
        monotonic_clock: &'a dyn MonotonicClock,
    ) -> Result<Self> {
        let manager = Self::with_clock(monotonic_clock);
        manager.synchronize(source)?;
        Ok(manager)
    }

    /// Start with invalid UTC; elapsed time is available immediately.
    pub fn unsynchronized() -> Self {
        Self::with_clock(&EMBASSY_CLOCK)
    }

    /// Start with invalid UTC and a platform-specific monotonic source.
    /// The default resolution constant describes Embassy; custom implementations
    /// must report their own resolution through `MonotonicClock::resolution`.
    pub fn with_clock(monotonic_clock: &'a dyn MonotonicClock) -> Self {
        Self {
            monotonic_clock,
            start_time: monotonic_clock.now(),
            realtime: Mutex::new(RefCell::new(RealtimeStatus {
                correction_nanoseconds: 0,
                synchronization: None,
            })),
        }
    }

    /// Monotonic time since the source's arbitrary epoch, independent of UTC.
    /// Use this epoch for absolute deadlines; use `uptime` for manager lifetime.
    pub fn monotonic_now(&self) -> Duration {
        self.monotonic_clock.now()
    }

    pub fn monotonic_resolution(&self) -> Duration {
        self.monotonic_clock.resolution()
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

    /// Signed Unix UTC, or Unsynchronized until an epoch has been supplied.
    /// May jump in either direction; never use for elapsed time. Overflow is an
    /// error rather than a wrapped timestamp; a later set can recover the clock.
    pub fn realtime_now(&self) -> Result<Timestamp> {
        self.realtime.lock(|state| {
            let state = state.borrow();
            if state.synchronization.is_none() {
                return Err(Error::Unsynchronized);
            }
            (self.monotonic_now().as_nanos() as i128)
                .checked_add(state.correction_nanoseconds)
                .map(Timestamp::from_nanos)
                .ok_or(Error::Overflow)
        })
    }

    /// Replace UTC and its provenance atomically at the current monotonic instant.
    /// Invalid samples or correction overflow leave all prior state untouched.
    /// This runtime-owned API does not grant guest users permission to change time.
    pub fn set_realtime(&self, sample: RealtimeSample) -> Result<()> {
        if sample.resolution.is_zero() {
            return Err(Error::InvalidResolution);
        }
        self.realtime.lock(|state| {
            let now = self.monotonic_now();
            let correction = sample
                .timestamp
                .as_nanos()
                .checked_sub(now.as_nanos() as i128)
                .ok_or(Error::Overflow)?;
            *state.borrow_mut() = RealtimeStatus {
                correction_nanoseconds: correction,
                synchronization: Some(Synchronization {
                    source: sample.source,
                    at_monotonic: now,
                    resolution: sample.resolution,
                    accuracy: sample.accuracy,
                }),
            };
            Ok(())
        })
    }

    /// Sample a trusted source explicitly; failed reads leave existing UTC intact.
    pub fn synchronize(&self, source: &dyn RealtimeSource) -> Result<()> {
        self.set_realtime(source.read()?)
    }

    pub fn status(&self) -> RealtimeStatus {
        self.realtime.lock(|state| *state.borrow())
    }

    /// Mark UTC invalid without disturbing the monotonic clock or its deadlines.
    pub fn invalidate(&self) {
        self.realtime.lock(|state| {
            *state.borrow_mut() = RealtimeStatus {
                correction_nanoseconds: 0,
                synchronization: None,
            };
        });
    }

    /// Nominal quantization; this remains queryable while UTC is invalid.
    pub fn realtime_resolution(&self) -> Duration {
        self.status()
            .synchronization
            .map_or(self.monotonic_resolution(), |sync| {
                sync.resolution.max(self.monotonic_resolution())
            })
    }
}

pub fn initialize(source: &dyn RealtimeSource) -> Result<Manager<'static>> {
    Manager::new(source)
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
