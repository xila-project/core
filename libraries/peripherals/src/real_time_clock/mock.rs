use core::cell::RefCell;

use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

use super::{Error, RealTimeClockController, Time};

struct State {
    time: Option<Time>,
    write_count: u32,
    fail_reads: bool,
    fail_writes: bool,
}

/// Simulated real-time clock. It records every write so tests can observe what
/// a driver or the write-back task requested, and can fail reads and writes on
/// demand.
pub struct MockRealTimeClock {
    state: Mutex<CriticalSectionRawMutex, RefCell<State>>,
}

impl MockRealTimeClock {
    /// A clock that holds `time` (`None` for no valid time), never failing.
    pub const fn new(time: Option<Time>) -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                time,
                write_count: 0,
                fail_reads: false,
                fail_writes: false,
            })),
        }
    }

    /// Replaces the time the clock holds.
    pub fn set_time(&self, time: Option<Time>) {
        self.state.lock(|state| state.borrow_mut().time = time);
    }

    /// The time the clock currently holds.
    pub fn time(&self) -> Option<Time> {
        self.state.lock(|state| state.borrow().time)
    }

    /// How many times `write` succeeded so far.
    pub fn write_count(&self) -> u32 {
        self.state.lock(|state| state.borrow().write_count)
    }

    /// When set, `read` fails with [`Error::Other`] instead of returning the time.
    pub fn fail_reads(&self, fail: bool) {
        self.state
            .lock(|state| state.borrow_mut().fail_reads = fail);
    }

    /// When set, `write` fails with [`Error::Other`] and records nothing.
    pub fn fail_writes(&self, fail: bool) {
        self.state
            .lock(|state| state.borrow_mut().fail_writes = fail);
    }
}

impl RealTimeClockController for MockRealTimeClock {
    fn read(&self) -> Result<Option<Time>, Error> {
        self.state.lock(|state| {
            let state = state.borrow();
            if state.fail_reads {
                Err(Error::Other)
            } else {
                Ok(state.time)
            }
        })
    }

    fn write(&self, time: Time) -> Result<(), Error> {
        self.state.lock(|state| {
            let mut state = state.borrow_mut();
            if state.fail_writes {
                return Err(Error::Other);
            }
            state.time = Some(time);
            state.write_count += 1;
            Ok(())
        })
    }
}
