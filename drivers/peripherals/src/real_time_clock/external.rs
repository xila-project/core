//! Generic external real-time clock chip lower half over the `rtcc` crate.
//!
//! Any chip driver that implements `rtcc::DateTimeAccess` (for example
//! `ds323x` 0.7 for the DS3231 and DS3232 on an I2C bus) plugs into Xila's
//! real-time clock device through [`ExternalRealTimeClock`]. `chrono` stays
//! `no_std` without default features, and `NaiveDateTime` is interpreted as UTC.
//!
//! No board ships such a chip today. To enable one, a board constructs the chip
//! driver on its I2C bus and hands it to the firmware:
//!
//! ```ignore
//! use drivers_peripherals::real_time_clock::ExternalRealTimeClock;
//! // ds323x 0.7: DS3231 at its fixed 7-bit address on an embedded-hal I2C bus.
//! let chip = ds323x::Ds323x::new(i2c_bus);
//! let clock = ExternalRealTimeClock::new(chip);
//! // Then either pass `&'static clock` as the board's real-time clock, or mount
//! // it directly: `mount_real_time_clock(vfs, task, &'static clock).await`.
//! ```
//!
//! The adapter always reports a valid time: `DateTimeAccess` has no validity
//! flag, so a chip whose oscillator stopped (the DS3231's OSF bit) still reads
//! a time. A board that needs that distinction wraps the chip and returns
//! `Ok(None)` from [`RealTimeClockController::read`] when the flag is set.

use core::cell::RefCell;

use peripherals::real_time_clock::{Error, RealTimeClockController, Time};
use rtcc::{DateTime, DateTimeAccess};
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

/// Adapter from an `rtcc` chip driver to Xila's real-time clock controller.
///
/// The chip is behind a mutex because `DateTimeAccess` takes `&mut self` while
/// the controller is shared between the device node and the write-back task.
/// The mutex is only held for the short take/put-back of the chip: a chip
/// transaction blocks on its I2C bus and must never run inside a critical
/// section, so the transaction runs with the chip taken out and a concurrent
/// access finds the slot empty and fails with [`Error::Busy`]. The chip is
/// stored by value; a board that also uses the chip elsewhere keeps it in an
/// `Arc` and implements `DateTimeAccess` on the guard.
pub struct ExternalRealTimeClock<D> {
    chip: Mutex<CriticalSectionRawMutex, RefCell<Option<D>>>,
}

impl<D> ExternalRealTimeClock<D> {
    pub const fn new(chip: D) -> Self {
        Self {
            chip: Mutex::new(RefCell::new(Some(chip))),
        }
    }

    /// Runs `f` with the chip taken out of its slot, so the (blocking) call
    /// never runs inside a critical section. A concurrent access finds the
    /// slot empty and fails with [`Error::Busy`].
    fn with_chip<R, E>(&self, f: impl FnOnce(&mut D) -> Result<R, E>) -> Result<R, Error> {
        let chip = self.chip.lock(|slot| slot.borrow_mut().take());
        let Some(mut chip) = chip else {
            return Err(Error::Busy);
        };
        let result = f(&mut chip);
        self.chip.lock(|slot| *slot.borrow_mut() = Some(chip));
        result.map_err(|_| Error::Other)
    }
}

impl<D: DateTimeAccess + Send + Sync> RealTimeClockController for ExternalRealTimeClock<D> {
    fn read(&self) -> Result<Option<Time>, Error> {
        let datetime = self.with_chip(|chip| chip.datetime())?;
        // `NaiveDateTime` is interpreted as UTC; both accessors are checked
        // conversions of the same value.
        let utc = datetime.and_utc();
        let mut seconds = utc.timestamp();
        let mut nanoseconds = utc.timestamp_subsec_nanos();
        if nanoseconds >= 1_000_000_000 {
            // chrono reports a leap second as a nanosecond count of one second
            // or more; fold it into the seconds instead of panicking in
            // `Time::new`.
            seconds += i64::from(nanoseconds / 1_000_000_000);
            nanoseconds %= 1_000_000_000;
        }
        Ok(Some(Time::new(seconds, nanoseconds)))
    }

    fn write(&self, time: Time) -> Result<(), Error> {
        let datetime = DateTime::from_timestamp(time.seconds, time.nanoseconds)
            .ok_or(Error::Invalid)?
            .naive_utc();
        self.with_chip(|chip| chip.set_datetime(&datetime))
    }
}
