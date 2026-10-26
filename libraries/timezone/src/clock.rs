use crate::{Error, LocalTime, Result, Zone};

impl Zone {
    /// Convert the system clock only when its UTC epoch is valid. Clock failures
    /// are preserved; timezone selection never sets or adjusts either core clock.
    pub fn now(self, clock: &time::Manager<'_>) -> Result<LocalTime> {
        let utc = clock.realtime_now().map_err(Error::Clock)?;
        self.utc_to_local(utc.as_nanos())
    }
}
