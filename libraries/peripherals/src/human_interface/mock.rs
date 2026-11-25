//! Mock human interface controller for tests of the upper half.

use portable_atomic::{AtomicBool, AtomicU8, Ordering};
use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use super::{Error, HumanInterfaceController, REPORT_MAX_BYTES};

/// How many reports a [`HumanInterfaceMock`] records before failing with `Error::Busy`.
pub const MOCK_MAX_REPORTS: usize = 64;

type RecordedReport = (u8, [u8; REPORT_MAX_BYTES], usize);

/// Records the reports sent to a [`HumanInterfaceController`] and answers the status
/// queries. Storage lives inline; nothing is allocated.
pub struct HumanInterfaceMock {
    sent: Mutex<CriticalSectionRawMutex, ([Option<RecordedReport>; MOCK_MAX_REPORTS], usize)>,
    connected: AtomicBool,
    indicators: AtomicU8,
    fail_with: Mutex<CriticalSectionRawMutex, Option<Error>>,
}

impl HumanInterfaceMock {
    /// New mock: not connected, no indicators.
    pub const fn new() -> Self {
        Self {
            sent: Mutex::new(([const { None }; MOCK_MAX_REPORTS], 0)),
            connected: AtomicBool::new(false),
            indicators: AtomicU8::new(0),
            fail_with: Mutex::new(None),
        }
    }

    fn sent(
        &self,
    ) -> MutexGuard<'_, CriticalSectionRawMutex, ([Option<RecordedReport>; MOCK_MAX_REPORTS], usize)>
    {
        self.sent
            .try_lock()
            .expect("human interface mock reports are already locked")
    }

    /// Set the `is_connected` answer.
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Relaxed);
    }

    /// Set the `indicators` answer.
    pub fn set_indicators(&self, indicators: u8) {
        self.indicators.store(indicators, Ordering::Relaxed);
    }

    /// Make `send_report` fail with `error` while set, to test partial writes.
    pub fn set_fail_with(&self, error: Option<Error>) {
        *self
            .fail_with
            .try_lock()
            .expect("human interface mock failure is already locked") = error;
    }

    /// How many reports were recorded.
    pub fn sent_len(&self) -> usize {
        self.sent().1
    }

    /// Report recorded at `index`, as `(identifier, report, length)`, or `None`.
    pub fn sent_at(&self, index: usize) -> Option<RecordedReport> {
        self.sent().0[index]
    }
}

impl Default for HumanInterfaceMock {
    fn default() -> Self {
        Self::new()
    }
}

impl HumanInterfaceController for HumanInterfaceMock {
    fn send_report(&mut self, identifier: u8, report: &[u8]) -> Result<(), Error> {
        let fail_with = *self
            .fail_with
            .try_lock()
            .expect("human interface mock failure is already locked");
        if let Some(error) = fail_with {
            return Err(error);
        }
        let mut sent = self.sent();
        if sent.1 == MOCK_MAX_REPORTS {
            return Err(Error::Busy);
        }
        let mut bytes = [0u8; REPORT_MAX_BYTES];
        bytes[..report.len()].copy_from_slice(report);
        let count = sent.1;
        sent.0[count] = Some((identifier, bytes, report.len()));
        sent.1 = count + 1;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn indicators(&self) -> u8 {
        self.indicators.load(Ordering::Relaxed)
    }
}
