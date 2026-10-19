#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Duration(u64);

impl Duration {
    pub const MINIMUM: Self = Self(u64::MIN);
    pub const MAXIMUM: Self = Self(u64::MAX);

    pub const fn from_seconds(seconds: u64) -> Self {
        Self(embassy_time::Duration::from_secs(seconds).as_ticks())
    }

    pub const fn from_milliseconds(milliseconds: u64) -> Self {
        Self(embassy_time::Duration::from_millis(milliseconds).as_ticks())
    }

    pub const fn from_microseconds(microseconds: u64) -> Self {
        Self(embassy_time::Duration::from_micros(microseconds).as_ticks())
    }

    pub const fn from_nanoseconds(nanoseconds: u64) -> Self {
        Self(embassy_time::Duration::from_nanos(nanoseconds).as_ticks())
    }

    pub const fn as_ticks(&self) -> u64 {
        self.0
    }

    pub const fn as_seconds(&self) -> u64 {
        embassy_time::Duration::from_ticks(self.0).as_secs()
    }

    pub const fn as_milliseconds(&self) -> u64 {
        embassy_time::Duration::from_ticks(self.0).as_millis()
    }

    pub const fn as_microseconds(&self) -> u64 {
        embassy_time::Duration::from_ticks(self.0).as_micros()
    }

    pub const fn into_smoltcp(self) -> smoltcp::time::Duration {
        smoltcp::time::Duration::from_micros(self.as_microseconds())
    }

    pub const fn into_embassy(self) -> embassy_time::Duration {
        embassy_time::Duration::from_ticks(self.0)
    }

    pub const fn from_embassy(value: embassy_time::Duration) -> Self {
        Self(value.as_ticks())
    }

    pub const fn from_smoltcp(value: smoltcp::time::Duration) -> Self {
        Self::from_microseconds(value.micros())
    }
}

impl From<embassy_time::Duration> for Duration {
    fn from(value: embassy_time::Duration) -> Self {
        Self::from_embassy(value)
    }
}

impl From<Duration> for embassy_time::Duration {
    fn from(value: Duration) -> Self {
        value.into_embassy()
    }
}

impl From<core::time::Duration> for Duration {
    fn from(value: core::time::Duration) -> Self {
        Self::from_microseconds(value.as_micros() as u64)
    }
}

impl From<Duration> for core::time::Duration {
    fn from(value: Duration) -> Self {
        core::time::Duration::from_micros(value.as_microseconds())
    }
}
