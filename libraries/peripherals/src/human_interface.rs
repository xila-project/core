//! Human interface emulation lower-half interface.
//!
//! A target implements [`HumanInterfaceController`] so that the device acts as a USB or
//! Bluetooth keyboard, media remote and mouse toward the computer it is connected to. A
//! lower half only needs this trait and [`REPORT_DESCRIPTOR`]: it sends the reports
//! described once by the descriptor. USB drivers prefix the report identifier byte to the
//! report bytes themselves; Bluetooth Low Energy HID over GATT sends the identifier out of
//! band, in the Report Reference descriptor of the report characteristic.
//!
//! The upper half (`drivers_peripherals::human_interface`) validates the message structure
//! of the device node and calls `send_report` with the report identifier and the report
//! bytes without the identifier byte; it converts the messages to reports. All validation
//! and error handling live there, not in the transports.
//!
//! The Linux `uinput` controller of the Platforms repository
//! (`drivers_std::human_interface::uinput`) is the reference implementation for testing on
//! a computer; later transports are USB HID (for example `embassy-usb` on the ESP32-S3 and
//! the nRF52840) and Bluetooth Low Energy HID over GATT (`esp-radio` / `nrf-sdc` with
//! `trouble-host`).
//!
//! The keyboard report uses the boot protocol byte layout (modifiers, reserved byte, six
//! key usages), but this descriptor does not declare boot protocol support. A lower half
//! that answers `SET_PROTOCOL(boot)` must drop the report identifier byte after the switch,
//! because boot reports carry no identifier.
//!
//! A lower half must never drop a report silently: when its queue is full it returns
//! [`Error::Busy`], and the caller sees `WouldBlock`.

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::HumanInterfaceMock;

/// Report identifier of the keyboard report. Equals the keyboard message kind byte.
pub const REPORT_IDENTIFIER_KEYBOARD: u8 = 1;
/// Report identifier of the consumer control report. Equals the consumer message kind byte.
pub const REPORT_IDENTIFIER_CONSUMER: u8 = 2;
/// Report identifier of the mouse report. Equals the mouse message kind byte.
pub const REPORT_IDENTIFIER_MOUSE: u8 = 3;
/// Report identifier of the system control report. Equals the system message kind byte.
pub const REPORT_IDENTIFIER_SYSTEM: u8 = 4;

/// Largest input report of [`REPORT_DESCRIPTOR`] in bytes: the 8-byte keyboard report.
pub const REPORT_MAX_BYTES: usize = 8;

/// Error of a human interface controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// No computer is connected and listening.
    NotConnected,
    /// The transport queue is full; the report was not accepted. A lower half must not
    /// drop a report silently.
    Busy,
    /// Any other transport failure.
    Other,
}

impl core::fmt::Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::NotConnected => "no computer is connected",
            Self::Busy => "human interface transport queue is full",
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
    /// Send one HID report to the computer. `identifier` is one of the
    /// `REPORT_IDENTIFIER_*` constants and `report` holds the report bytes without the
    /// identifier byte.
    fn send_report(&mut self, identifier: u8, report: &[u8]) -> Result<(), Error>;

    /// `true` when a computer is connected and listening.
    fn is_connected(&self) -> bool;

    /// Keyboard indicator bitmask from the computer: one bit per LED (num lock, caps
    /// lock, scroll lock), in the order of the LED usages of [`REPORT_DESCRIPTOR`].
    fn indicators(&self) -> u8;
}

/// HID report descriptor of the composite emulation device: one keyboard (report
/// identifier 1, 8-byte boot-layout input report plus a one-byte LED output report), one
/// consumer control (identifier 2, one 16-bit usage), one mouse (identifier 3, buttons,
/// x, y, wheel, pan) and one system control (identifier 4, one usage byte). USB and
/// Bluetooth Low Energy lower halves both use it.
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
    0x26, 0xFF, 0x00, //   Logical Maximum (255)
    0x19, 0x00, //   Usage Minimum (0)
    0x2A, 0xFF, 0x00, //   Usage Maximum (255)
    0x81, 0x00, //   Input (Data, Array, Absolute): six key usages
    0x95, 0x05, //   Report Count (5)
    0x75, 0x01, //   Report Size (1)
    0x15, 0x00, //   Logical Minimum (0)
    0x25, 0x01, //   Logical Maximum (1)
    0x05, 0x08, //   Usage Page (LEDs)
    0x19, 0x01, //   Usage Minimum (Num Lock)
    0x29, 0x05, //   Usage Maximum (Kana)
    0x91, 0x02, //   Output (Data, Variable, Absolute): five LED bits
    0x95, 0x01, //   Report Count (1)
    0x75, 0x03, //   Report Size (3)
    0x91, 0x01, //   Output (Constant): LED padding, 8 bits in total
    0xC0, // End Collection
    // Consumer control, report identifier 2.
    0x05, 0x0C, // Usage Page (Consumer)
    0x09, 0x01, // Usage (Consumer Control)
    0xA1, 0x01, // Collection (Application)
    0x85, 0x02, //   Report ID (2)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xFF, 0x03, //   Logical Maximum (1023)
    0x19, 0x00, //   Usage Minimum (0)
    0x2A, 0xFF, 0x03, //   Usage Maximum (1023)
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
    0x16, 0x80, 0xFF, //     Logical Minimum (-128)
    0x25, 0x7F, //     Logical Maximum (127)
    0x75, 0x08, //     Report Size (8)
    0x95, 0x03, //     Report Count (3)
    0x81, 0x06, //     Input (Data, Variable, Relative): x, y, wheel
    0x05, 0x0C, //     Usage Page (Consumer)
    0x0A, 0x38, 0x02, //     Usage (AC Pan, 0x0238)
    0x16, 0x80, 0xFF, //     Logical Minimum (-128)
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
    0x15, 0x01, //   Logical Minimum (1)
    0x26, 0xB7, 0x00, //   Logical Maximum (0xB7)
    0x19, 0x01, //   Usage Minimum (1)
    0x29, 0xB7, //   Usage Maximum (0xB7)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x01, //   Report Count (1)
    0x81, 0x00, //   Input (Data, Array, Absolute): one usage, 0 = release
    0xC0, // End Collection
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_identifiers_are_the_message_kinds() {
        assert_eq!(REPORT_IDENTIFIER_KEYBOARD, 1);
        assert_eq!(REPORT_IDENTIFIER_CONSUMER, 2);
        assert_eq!(REPORT_IDENTIFIER_MOUSE, 3);
        assert_eq!(REPORT_IDENTIFIER_SYSTEM, 4);
    }

    #[test]
    fn descriptor_declares_each_report_identifier() {
        for identifier in [
            REPORT_IDENTIFIER_KEYBOARD,
            REPORT_IDENTIFIER_CONSUMER,
            REPORT_IDENTIFIER_MOUSE,
            REPORT_IDENTIFIER_SYSTEM,
        ] {
            assert!(
                REPORT_DESCRIPTOR
                    .windows(2)
                    .any(|window| window == [0x85, identifier]),
                "report identifier {identifier} missing from the descriptor"
            );
        }
    }
}
