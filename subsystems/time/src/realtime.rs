use crate::{Error, Result};
use core::time::Duration;

const NANOS_PER_SECOND: i128 = 1_000_000_000;

/// Signed nanoseconds since 1970-01-01 00:00:00 UTC, without timezone or DST.
/// Leap seconds follow the supplying source's Unix/POSIX time convention.
/// The full i128 range is representable; conversions and clock arithmetic are checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(i128);

impl Timestamp {
    pub const fn from_nanos(nanoseconds: i128) -> Self {
        Self(nanoseconds)
    }

    pub const fn as_nanos(self) -> i128 {
        self.0
    }

    /// Whole Unix seconds, rounding toward the past (including before the epoch).
    pub fn try_as_seconds(self) -> Result<i64> {
        self.0
            .div_euclid(NANOS_PER_SECOND)
            .try_into()
            .map_err(|_| Error::Overflow)
    }

    /// Convert for consumers whose timestamp format excludes pre-epoch dates.
    pub fn try_into_duration(self) -> Result<Duration> {
        if self.0 < 0 {
            return Err(Error::Overflow);
        }
        let seconds = u64::try_from(self.0 / NANOS_PER_SECOND).map_err(|_| Error::Overflow)?;
        Ok(Duration::new(seconds, (self.0 % NANOS_PER_SECOND) as u32))
    }
}

impl From<Duration> for Timestamp {
    fn from(value: Duration) -> Self {
        // Duration's u64 seconds always fit in signed i128 nanoseconds.
        Self(value.as_nanos() as i128)
    }
}

/// Provenance claimed by a trusted runtime caller, not an authentication mechanism.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSource {
    Host,
    Rtc,
    Manual,
    Network,
}

/// An epoch sample supplied by a platform or synchronization service.
///
/// `resolution` is the source's quantization, not an accuracy guarantee. `accuracy`
/// is a source-reported uncertainty at sampling time; None means unknown. Neither
/// field promises long-term accuracy: oscillator drift and suspend accumulate error.
/// The caller must account for acquisition/transport delay before submitting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealtimeSample {
    pub timestamp: Timestamp,
    pub source: ClockSource,
    pub resolution: Duration,
    pub accuracy: Option<Duration>,
}

/// A trustworthy Unix epoch source: RTC driver, host, manual input or network service.
///
/// Reading is explicit, only at bootstrap or `Manager::synchronize`; realtime reads
/// never poll the source. A source must return an error rather than fabricate an
/// epoch when unavailable. The sample is anchored when read completes; slow sources
/// should compensate latency and include its uncertainty in the supplied accuracy.
pub trait RealtimeSource: Send + Sync {
    fn read(&self) -> Result<RealtimeSample>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Synchronization {
    pub source: ClockSource,
    pub at_monotonic: Duration,
    pub resolution: Duration,
    pub accuracy: Option<Duration>,
}

/// No synchronization means UTC is invalid, regardless of the monotonic reading.
/// The correction starts at zero and is reset by explicit invalidation; it is
/// replaced (never accumulated) by each successful set operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealtimeStatus {
    pub correction_nanoseconds: i128,
    pub synchronization: Option<Synchronization>,
}
