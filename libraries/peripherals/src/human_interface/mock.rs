use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use super::{Error, HumanInterfaceController, Report};

#[derive(Default)]
struct MockState {
    connected: bool,
    indicators: u8,
    last_report: Option<Report>,
    report_count: usize,
}

/// Deterministic human interface controller for tests. It records the last report and the
/// report count, and answers the status queries from settable fields.
pub struct HumanInterfaceMock {
    state: Mutex<CriticalSectionRawMutex, MockState>,
}

impl Default for HumanInterfaceMock {
    fn default() -> Self {
        Self::new()
    }
}

impl HumanInterfaceMock {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(MockState {
                connected: false,
                indicators: 0,
                last_report: None,
                report_count: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, CriticalSectionRawMutex, MockState> {
        self.state
            .try_lock()
            .expect("human interface mock is already locked")
    }

    /// Set the connection state reported by `is_connected`.
    pub fn set_connected(&self, connected: bool) {
        self.lock().connected = connected;
    }

    /// Set the keyboard indicator bitmask reported by `indicators`.
    pub fn set_indicators(&self, indicators: u8) {
        self.lock().indicators = indicators;
    }

    /// Number of reports received so far.
    pub fn report_count(&self) -> usize {
        self.lock().report_count
    }

    /// The last report received, if any.
    pub fn last_report(&self) -> Option<Report> {
        self.lock().last_report
    }
}

impl HumanInterfaceController for HumanInterfaceMock {
    fn send_report(&mut self, identifier: u8, report: &[u8]) -> Result<(), Error> {
        let mut state = self.lock();
        state.last_report = Report::new(identifier, report);
        state.report_count += 1;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.lock().connected
    }

    fn indicators(&self) -> u8 {
        self.lock().indicators
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::*;
    use crate::human_interface::{REPORT_ID_KEYBOARD, REPORT_ID_MOUSE};
    use device::human_interface::Message;

    #[test]
    fn mock_records_reports_and_answers_status_queries() {
        let mut mock = HumanInterfaceMock::new();
        assert!(!mock.is_connected());
        assert_eq!(mock.indicators(), 0);
        assert_eq!(mock.report_count(), 0);
        assert_eq!(mock.last_report(), None);

        mock.set_connected(true);
        mock.set_indicators(device::human_interface::INDICATOR_CAPS_LOCK);
        assert!(mock.is_connected());
        assert_eq!(mock.indicators(), 0x02);

        let report = Report::from_message(&Message::Keyboard {
            modifiers: device::human_interface::Modifiers::empty(),
            keys: [device::human_interface::keyboard::A, 0, 0, 0, 0, 0],
        });
        assert_eq!(report.identifier(), REPORT_ID_KEYBOARD);
        mock.send_report(report.identifier(), report.bytes())
            .unwrap();
        assert_eq!(mock.last_report().unwrap().identifier(), REPORT_ID_KEYBOARD);
        let report = Report::from_message(&Message::Mouse {
            buttons: device::human_interface::MouseButtons::LEFT,
            x: 5,
            y: -5,
            wheel: 0,
            pan: 0,
        });
        mock.send_report(report.identifier(), report.bytes())
            .unwrap();

        assert_eq!(mock.report_count(), 2);
        let last = mock.last_report().unwrap();
        assert_eq!(last.identifier(), REPORT_ID_MOUSE);
        assert_eq!(last.bytes(), &[0x01, 0x05, 0xFB, 0x00, 0x00]);
    }
}
