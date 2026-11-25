//! Human interface emulation lower-half interface.
//!
//! A target implements [`HumanInterfaceController`] so that the device acts as a USB or
//! Bluetooth keyboard, media remote and mouse toward the computer it is connected to. Every
//! transport sends the same HID reports, described once by [`REPORT_DESCRIPTOR`]:
//! [`Report::from_message`] converts a `device::human_interface::Message` into the report
//! identifier and the report bytes WITHOUT the identifier byte. USB drivers prefix the
//! identifier byte to the report bytes themselves; Bluetooth Low Energy HID over GATT sends
//! the identifier out of band, in the Report Reference descriptor of the report
//! characteristic.
//!
//! The upper half (`drivers_peripherals::human_interface`) validates the 8-byte messages of
//! the device node and calls `send_report`; all validation and error handling live there, not
//! in the transports. The Linux `uinput` controller of the Platforms repository
//! (`drivers_std::human_interface::uinput`) is the reference implementation for testing on a
//! computer; later transports are USB HID (for example `embassy-usb` on the ESP32-S3 and the
//! nRF52840) and Bluetooth Low Energy HID over GATT (`esp-radio` / `nrf-sdc` with
//! `trouble-host`).

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::HumanInterfaceMock;

use device::human_interface::Message;

/// Report identifier of the keyboard report. Equals the keyboard message kind byte.
pub const REPORT_ID_KEYBOARD: u8 = device::human_interface::KIND_KEYBOARD;
/// Report identifier of the consumer control report. Equals the consumer message kind byte.
pub const REPORT_ID_CONSUMER: u8 = device::human_interface::KIND_CONSUMER;
/// Report identifier of the mouse report. Equals the mouse message kind byte.
pub const REPORT_ID_MOUSE: u8 = device::human_interface::KIND_MOUSE;
/// Report identifier of the system control report. Equals the system message kind byte.
pub const REPORT_ID_SYSTEM: u8 = device::human_interface::KIND_SYSTEM;

/// Largest report of [`REPORT_DESCRIPTOR`] in bytes: the 8-byte keyboard input report.
pub const REPORT_MAX_BYTES: usize = 8;

/// Error of a human interface controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No computer is connected and listening.
    NotConnected,
    /// Any other transport failure.
    Other,
}

impl core::fmt::Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::NotConnected => "no computer is connected",
            Self::Other => "human interface report failed",
        })
    }
}

impl core::error::Error for Error {}

/// Hardware-facing controller of the human interface emulation device.
///
/// The upper half validates every message, so `send_report` only receives a known report
/// identifier and a report of the matching length. Each method is a single synchronous
/// attempt and must return promptly, because it runs on the executor.
pub trait HumanInterfaceController {
    /// Send one HID report to the computer. `identifier` is one of the `REPORT_ID_*`
    /// constants and `report` holds the report bytes without the identifier byte.
    fn send_report(&mut self, identifier: u8, report: &[u8]) -> Result<(), Error>;

    /// `true` when a computer is connected and listening.
    fn is_connected(&self) -> bool;

    /// Keyboard indicator bitmask from the computer, `device::human_interface::INDICATOR_*`
    /// bits.
    fn indicators(&self) -> u8;
}

/// One HID report: the report identifier and the report bytes without the identifier byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    identifier: u8,
    bytes: [u8; REPORT_MAX_BYTES],
    length: usize,
}

impl Report {
    /// Build a report from its identifier and its bytes, without the identifier byte.
    /// Returns `None` when `bytes` is longer than [`REPORT_MAX_BYTES`].
    pub fn new(identifier: u8, bytes: &[u8]) -> Option<Self> {
        if bytes.len() > REPORT_MAX_BYTES {
            return None;
        }
        let mut stored = [0u8; REPORT_MAX_BYTES];
        stored[..bytes.len()].copy_from_slice(bytes);
        Some(Self {
            identifier,
            bytes: stored,
            length: bytes.len(),
        })
    }

    /// Convert a message into its HID report, as described by [`REPORT_DESCRIPTOR`].
    pub fn from_message(message: &Message) -> Self {
        match message {
            Message::Keyboard { modifiers, keys } => {
                // Boot layout: modifiers, reserved byte, six key usages.
                let mut bytes = [0u8; REPORT_MAX_BYTES];
                bytes[0] = modifiers.bits();
                bytes[2..].copy_from_slice(keys);
                Self::new(REPORT_ID_KEYBOARD, &bytes).expect("keyboard report fits")
            }
            Message::Consumer { usage } => {
                Self::new(REPORT_ID_CONSUMER, &usage.to_le_bytes()).expect("consumer report fits")
            }
            Message::Mouse {
                buttons,
                x,
                y,
                wheel,
                pan,
            } => Self::new(
                REPORT_ID_MOUSE,
                &[buttons.bits(), *x as u8, *y as u8, *wheel as u8, *pan as u8],
            )
            .expect("mouse report fits"),
            Message::System { usage } => {
                Self::new(REPORT_ID_SYSTEM, &[*usage]).expect("system report fits")
            }
        }
    }

    /// The report identifier, one of the `REPORT_ID_*` constants.
    pub const fn identifier(&self) -> u8 {
        self.identifier
    }

    /// The report bytes, without the identifier byte.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

/// HID report descriptor of the composite emulation device: one keyboard (report
/// identifier 1, 8-byte boot input report plus a one-byte LED output report), one consumer
/// control (identifier 2, one 16-bit usage), one mouse (identifier 3, buttons, x, y, wheel,
/// pan) and one system control (identifier 4, one usage byte). USB and Bluetooth Low Energy
/// lower halves both use it.
pub const REPORT_DESCRIPTOR: &[u8] = &[
    // Keyboard, report identifier 1.
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x06, // Usage (Keyboard)
    0xA1, 0x01, // Collection (Application)
    0x85, 0x01, //   Report ID (1)
    0x05, 0x07, //   Usage Page (Keyboard/Keypad)
    0x19, 0xE0, //   Usage Minimum (0xE0, left control)
    0x29, 0xE7, //   Usage Maximum (0xE7, right GUI)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x75, 0x01, //   Report Size (1)
    0x95, 0x08, //   Report Count (8)
    0x81, 0x02, //   Input (Data, Variable, Absolute): modifier byte
    0x95, 0x01, //   Report Count (1)
    0x75, 0x08, //   Report Size (8)
    0x81, 0x01, //   Input (Constant): reserved byte
    0x95, 0x06, //   Report Count (6)
    0x75, 0x08, //   Report Size (8)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x65, //   Logical Maximum (101)
    0x19, 0x00, //   Usage Minimum (0)
    0x29, 0x65, //   Usage Maximum (101)
    0x81, 0x00, //   Input (Data, Array, Absolute): six key usages
    0x95, 0x01, //   Report Count (1)
    0x75, 0x08, //   Report Size (8)
    0x05, 0x08, //   Usage Page (LEDs)
    0x19, 0x01, //   Usage Minimum (Num Lock)
    0x29, 0x05, //   Usage Maximum (Kana)
    0x91, 0x02, //   Output (Data, Variable, Absolute): LED report
    0x95, 0x01, //   Report Count (1)
    0x75, 0x03, //   Report Size (3)
    0x91, 0x01, //   Output (Constant): LED padding
    0xC0, // End Collection
    // Consumer control, report identifier 2.
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x85, 0x02, //   Report ID (2)
    0x15, 0x00, //   Logical Minimum (0)
    0x27, 0xFF, 0xFF, 0x00, 0x00, //   Logical Maximum (65535)
    0x19, 0x00, //   Usage Minimum (0)
    0x2A, 0xFF, 0xFF, //   Usage Maximum (65535)
    0x75, 0x10, //   Report Size (16)
    0x95, 0x01, //   Report Count (1)
    0x81, 0x00, //   Input (Data, Array, Absolute): one 16-bit usage, 0 = release
    0xC0, // End Collection
    // Mouse, report identifier 3.
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x02, // Usage (Mouse)
    0xA1, 0x01, // Collection (Application)
    0x85, 0x03, //   Report ID (3)
    0x09, 0x01, //   Usage (Pointer)
    0xA1, 0x00, //   Collection (Physical)
    0x05, 0x09, //     Usage Page (Button)
    0x19, 0x01, //     Usage Minimum (Button 1)
    0x29, 0x03, //     Usage Maximum (Button 3)
    0x15, 0x00, //     Logical Minimum (0)
    0x25, 0x01, //     Logical Maximum (1)
    0x95, 0x03, //     Report Count (3)
    0x75, 0x01, //     Report Size (1)
    0x81, 0x02, //     Input (Data, Variable, Absolute): buttons
    0x95, 0x01, //     Report Count (1)
    0x75, 0x05, //     Report Size (5)
    0x81, 0x01, //     Input (Constant): button padding
    0x05, 0x01, //     Usage Page (Generic Desktop)
    0x09, 0x30, //     Usage (X)
    0x09, 0x31, //     Usage (Y)
    0x09, 0x38, //     Usage (Wheel)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x03, //     Report Count (3)
    0x81, 0x06, //     Input (Data, Variable, Relative): x, y, wheel
    0x05, 0x0C, //     Usage Page (Consumer)
    0x09, 0x38, //     Usage (AC Pan)
    0x15, 0x81, //     Logical Minimum (-127)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x01, //     Report Count (1)
    0x81, 0x06, //     Input (Data, Variable, Relative): horizontal pan
    0xC0, //   End Collection (Physical)
    0xC0, // End Collection (Application)
    // System control, report identifier 4.
    0x05, 0x01, // Usage Page (Generic Desktop)
    0x09, 0x80, // Usage (System Control)
    0xA1, 0x01, // Collection (Application)
    0x85, 0x04, //   Report ID (4)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x19, 0x81, //   Usage Minimum (0x81, System Power Down)
    0x29, 0x83, //   Usage Maximum (0x83, System Wake Up)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x01, //   Report Count (1)
    0x81, 0x00, //   Input (Data, Array, Absolute): one usage, 0 = release
    0xC0, // End Collection
];

#[cfg(test)]
mod tests {
    use super::*;
    use device::human_interface::{Modifiers, MouseButtons, consumer, keyboard, system};

    #[test]
    fn report_identifiers_equal_the_message_kind_bytes() {
        assert_eq!(REPORT_ID_KEYBOARD, 1);
        assert_eq!(REPORT_ID_CONSUMER, 2);
        assert_eq!(REPORT_ID_MOUSE, 3);
        assert_eq!(REPORT_ID_SYSTEM, 4);
    }

    #[test]
    fn descriptor_declares_each_report_identifier() {
        for identifier in [
            REPORT_ID_KEYBOARD,
            REPORT_ID_CONSUMER,
            REPORT_ID_MOUSE,
            REPORT_ID_SYSTEM,
        ] {
            assert!(
                REPORT_DESCRIPTOR
                    .windows(2)
                    .any(|window| window == [0x85, identifier]),
                "report identifier {identifier} missing from the descriptor"
            );
        }
    }

    #[test]
    fn keyboard_report_uses_the_boot_layout_without_the_identifier_byte() {
        let message = Message::Keyboard {
            modifiers: Modifiers::LEFT_CONTROL.union(Modifiers::RIGHT_SHIFT),
            keys: [keyboard::A, keyboard::F5, 0, 0, 0, 0],
        };
        let report = Report::from_message(&message);
        assert_eq!(report.identifier(), REPORT_ID_KEYBOARD);
        assert_eq!(
            report.bytes(),
            &[0b0010_0001, 0x00, keyboard::A, keyboard::F5, 0, 0, 0, 0]
        );
        assert_eq!(report.bytes().len(), 8);
    }

    #[test]
    fn consumer_report_is_one_16_bit_usage_without_the_identifier_byte() {
        let report = Report::from_message(&Message::Consumer {
            usage: consumer::PLAY_PAUSE,
        });
        assert_eq!(report.identifier(), REPORT_ID_CONSUMER);
        assert_eq!(report.bytes(), &[0xCD, 0x00]);

        let release = Report::from_message(&Message::Consumer { usage: 0 });
        assert_eq!(release.bytes(), &[0x00, 0x00]);
    }

    #[test]
    fn mouse_report_holds_buttons_and_movements_without_the_identifier_byte() {
        let report = Report::from_message(&Message::Mouse {
            buttons: MouseButtons::LEFT.union(MouseButtons::MIDDLE),
            x: -3,
            y: 120,
            wheel: -1,
            pan: 2,
        });
        assert_eq!(report.identifier(), REPORT_ID_MOUSE);
        assert_eq!(report.bytes(), &[0b0000_0101, 0xFD, 0x78, 0xFF, 0x02]);
    }

    #[test]
    fn system_report_is_one_usage_byte_without_the_identifier_byte() {
        for (usage, byte) in [
            (system::POWER_DOWN, 0x81),
            (system::SLEEP, 0x82),
            (system::WAKE_UP, 0x83),
            (0, 0x00),
        ] {
            let report = Report::from_message(&Message::System { usage });
            assert_eq!(report.identifier(), REPORT_ID_SYSTEM);
            assert_eq!(report.bytes(), &[byte]);
        }
    }

    #[test]
    fn report_rejects_oversized_bytes() {
        assert!(Report::new(REPORT_ID_KEYBOARD, &[0u8; REPORT_MAX_BYTES + 1]).is_none());
        assert!(Report::new(REPORT_ID_SYSTEM, &[0u8; 1]).is_some());
        let report = Report::new(REPORT_ID_MOUSE, &[1, 2, 3]).unwrap();
        assert_eq!(report.bytes(), &[1, 2, 3]);
    }
}
