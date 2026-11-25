//! Human interface emulation device contract (`/devices/human_interface/emulation`, kind `b'h'`).
//!
//! A Xila device can act as a USB or Bluetooth keyboard, media remote and mouse toward the
//! computer it is connected to (shortcuts, screenshots, play/pause, volume). An application
//! writes messages to the device node. The upper half
//! (`drivers_peripherals::human_interface`) validates the messages and forwards HID reports
//! to the target controller (`peripherals::human_interface::HumanInterfaceController`), which
//! every USB and Bluetooth Low Energy lower half implements. The shared HID report descriptor
//! and the conversion from [`Message`] to report live in `peripherals::human_interface`.
//!
//! # File contract
//!
//! The node is write-only for reports: `read` fails with `Unsupported`. The connection
//! status is a control command, which WebAssembly guests reach through the `fd_control`
//! import of the Wasm host.
//!
//! ## Write
//!
//! Each `write` contains one or more messages of exactly [`MESSAGE_SIZE`] (8) bytes. A write
//! whose length is not a multiple of 8, or that contains an unknown kind in byte 0, fails
//! with an invalid-input error. Reserved bytes of a message are ignored on decode and written
//! as zero on encode.
//!
//! Byte 0 of a message is the kind:
//!
//! | Kind | Name             | Byte 1                                             | Bytes 2 to 7                                                                |
//! |------|------------------|----------------------------------------------------|-----------------------------------------------------------------------------|
//! | `1`  | keyboard         | modifier bitmask, see [`Modifiers`]               | up to six key usages from the HID Keyboard/Keypad page (0x07), `0` = none   |
//! | `2`  | consumer control | consumer usage (HID page 0x0C), little endian, `0` = release | `0`                                                                |
//! | `3`  | mouse            | buttons bitmask, see [`MouseButtons`]              | x (i8), y (i8), wheel (i8), pan (i8) in bytes 2 to 5, `0` in bytes 6 and 7   |
//! | `4`  | system control   | usage from the HID Generic Desktop page: `0x81` power down, `0x82` sleep, `0x83` wake up, `0` = release | `0` |
//!
//! A keyboard message is the full pressed state: an all-zero keyboard message releases all
//! keys.
//!
//! ## Control commands (kind `b'h'`)
//!
//! | Number | Command     | Direction | Input        | Output      | Effect                                                        |
//! |--------|-------------|-----------|--------------|-------------|---------------------------------------------------------------|
//! | `0x00` | `GET_STATUS` | Read      | `VersionAbi` | `StatusAbi` | Returns the connection state and the keyboard LED bitmask.   |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and carries zeroed
//! `reserved` fields. A wrong version or a non-zero reserved field is `InvalidArgument`.

use core::fmt;
use core::ops::{BitAnd, BitOr};

use file_system::{ControlCommand, define_command};

/// Path of the emulation device node in the virtual file system.
pub const EMULATION_PATH: &str = "/devices/human_interface/emulation";

/// Size of one message in bytes.
pub const MESSAGE_SIZE: usize = 8;

/// Kind byte of a keyboard message. Also the report identifier of the keyboard report.
pub const KIND_KEYBOARD: u8 = 1;
/// Kind byte of a consumer control message. Also the report identifier of the consumer report.
pub const KIND_CONSUMER: u8 = 2;
/// Kind byte of a mouse message. Also the report identifier of the mouse report.
pub const KIND_MOUSE: u8 = 3;
/// Kind byte of a system control message. Also the report identifier of the system report.
pub const KIND_SYSTEM: u8 = 4;

/// Number of key usages in one keyboard message.
pub const KEY_COUNT: usize = 6;

/// Version of every control ABI struct in this module.
pub const ABI_VERSION: u16 = 1;

/// Keyboard indicator bit 0: num lock is on.
pub const INDICATOR_NUM_LOCK: u8 = 1 << 0;
/// Keyboard indicator bit 1: caps lock is on.
pub const INDICATOR_CAPS_LOCK: u8 = 1 << 1;
/// Keyboard indicator bit 2: scroll lock is on.
pub const INDICATOR_SCROLL_LOCK: u8 = 1 << 2;

/// Error of the message protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    /// The input is not a valid message: an unknown kind, a length that is not a multiple of
    /// [`MESSAGE_SIZE`].
    InvalidInput,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput => write!(formatter, "invalid human interface message"),
        }
    }
}

impl core::error::Error for Error {}

/// One message of the emulation device.
///
/// A message is the full state of one function: a keyboard message lists every pressed key,
/// so an all-zero keyboard message releases all keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Message {
    /// Keyboard state: modifier bitmask and up to six key usages from the HID
    /// Keyboard/Keypad page (0x07), for example [`keyboard::A`]. `0` marks an empty slot.
    Keyboard {
        /// Modifier bitmask, see [`Modifiers`].
        modifiers: Modifiers,
        /// Up to six pressed key usages, `0` = none.
        keys: [u8; KEY_COUNT],
    },
    /// Consumer control usage from the HID Consumer page (0x0C), for example
    /// [`consumer::PLAY_PAUSE`]. `0` releases the control.
    Consumer {
        /// Consumer usage, little endian on the wire.
        usage: u16,
    },
    /// Mouse movement and buttons.
    Mouse {
        /// Buttons bitmask, see [`MouseButtons`].
        buttons: MouseButtons,
        /// Horizontal movement in pixels, positive to the right.
        x: i8,
        /// Vertical movement in pixels, positive down.
        y: i8,
        /// Vertical wheel steps, positive up.
        wheel: i8,
        /// Horizontal pan steps, positive right.
        pan: i8,
    },
    /// System control usage from the HID Generic Desktop page: [`system::POWER_DOWN`],
    /// [`system::SLEEP`], [`system::WAKE_UP`]. `0` releases the control.
    System {
        /// System usage.
        usage: u8,
    },
}

impl Message {
    /// Encode the message as its 8 wire bytes, kind in byte 0.
    pub fn encode(&self) -> [u8; MESSAGE_SIZE] {
        let mut bytes = [0u8; MESSAGE_SIZE];
        match self {
            Self::Keyboard { modifiers, keys } => {
                bytes[0] = KIND_KEYBOARD;
                bytes[1] = modifiers.bits();
                bytes[2..].copy_from_slice(keys);
            }
            Self::Consumer { usage } => {
                bytes[0] = KIND_CONSUMER;
                bytes[1..3].copy_from_slice(&usage.to_le_bytes());
            }
            Self::Mouse {
                buttons,
                x,
                y,
                wheel,
                pan,
            } => {
                bytes[0] = KIND_MOUSE;
                bytes[1] = buttons.bits();
                bytes[2] = *x as u8;
                bytes[3] = *y as u8;
                bytes[4] = *wheel as u8;
                bytes[5] = *pan as u8;
            }
            Self::System { usage } => {
                bytes[0] = KIND_SYSTEM;
                bytes[1] = *usage;
            }
        }
        bytes
    }

    /// Decode one 8-byte message. An unknown kind in byte 0 fails with
    /// [`Error::InvalidInput`]; reserved bytes are ignored.
    pub fn decode(bytes: &[u8; MESSAGE_SIZE]) -> Result<Self, Error> {
        match bytes[0] {
            KIND_KEYBOARD => Ok(Self::Keyboard {
                modifiers: Modifiers::from_bits_retain(bytes[1]),
                keys: bytes[2..]
                    .try_into()
                    .expect("slice of MESSAGE_SIZE - 2 bytes"),
            }),
            KIND_CONSUMER => Ok(Self::Consumer {
                usage: u16::from_le_bytes([bytes[1], bytes[2]]),
            }),
            KIND_MOUSE => Ok(Self::Mouse {
                buttons: MouseButtons::from_bits_retain(bytes[1]),
                x: bytes[2] as i8,
                y: bytes[3] as i8,
                wheel: bytes[4] as i8,
                pan: bytes[5] as i8,
            }),
            KIND_SYSTEM => Ok(Self::System { usage: bytes[1] }),
            _ => Err(Error::InvalidInput),
        }
    }
}

/// Iterator over the messages of one write.
///
/// [`Messages::new`] checks that the length of the write is a multiple of [`MESSAGE_SIZE`];
/// every item decodes one message and fails with [`Error::InvalidInput`] on an unknown kind.
#[derive(Clone, Debug)]
pub struct Messages<'a> {
    remaining: &'a [u8],
}

impl<'a> Messages<'a> {
    /// Validate the write and return its message iterator.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        if !bytes.len().is_multiple_of(MESSAGE_SIZE) {
            return Err(Error::InvalidInput);
        }
        Ok(Self { remaining: bytes })
    }
}

impl Iterator for Messages<'_> {
    type Item = Result<Message, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let (head, tail) = self.remaining.split_at(MESSAGE_SIZE);
        self.remaining = tail;
        Some(Message::decode(
            head.try_into().expect("MESSAGE_SIZE bytes"),
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.remaining.len() / MESSAGE_SIZE;
        (count, Some(count))
    }
}

impl ExactSizeIterator for Messages<'_> {}
impl core::iter::FusedIterator for Messages<'_> {}

/// Keyboard modifier bitmask, HID standard order: left control, left shift, left alt, left
/// GUI, then the right ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifier pressed.
    pub const NONE: Self = Self(0);
    /// Left control.
    pub const LEFT_CONTROL: Self = Self(1 << 0);
    /// Left shift.
    pub const LEFT_SHIFT: Self = Self(1 << 1);
    /// Left alt.
    pub const LEFT_ALT: Self = Self(1 << 2);
    /// Left GUI (Windows, command, super).
    pub const LEFT_GUI: Self = Self(1 << 3);
    /// Right control.
    pub const RIGHT_CONTROL: Self = Self(1 << 4);
    /// Right shift.
    pub const RIGHT_SHIFT: Self = Self(1 << 5);
    /// Right alt (alt gr).
    pub const RIGHT_ALT: Self = Self(1 << 6);
    /// Right GUI.
    pub const RIGHT_GUI: Self = Self(1 << 7);
    /// Every modifier bit.
    pub const ALL: Self = Self(0xFF);

    /// Empty bitmask.
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Bitmask from raw bits; unknown bits are retained.
    pub const fn from_bits_retain(bits: u8) -> Self {
        Self(bits)
    }

    /// Raw bits of the bitmask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// `true` when no bit is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `true` when every bit of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// `true` when at least one bit is set in both.
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Union of two bitmasks.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Set the bits of `other`.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Clear the bits of `other`.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl BitAnd for Modifiers {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// Mouse buttons bitmask: left, right, middle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MouseButtons(u8);

impl MouseButtons {
    /// No button pressed.
    pub const NONE: Self = Self(0);
    /// Left button.
    pub const LEFT: Self = Self(1 << 0);
    /// Right button.
    pub const RIGHT: Self = Self(1 << 1);
    /// Middle button.
    pub const MIDDLE: Self = Self(1 << 2);
    /// Every button bit.
    pub const ALL: Self = Self(0x07);

    /// Empty bitmask.
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Bitmask from raw bits; unknown bits are retained.
    pub const fn from_bits_retain(bits: u8) -> Self {
        Self(bits)
    }

    /// Raw bits of the bitmask.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// `true` when no bit is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// `true` when every bit of `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// `true` when at least one bit is set in both.
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Union of two bitmasks.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Set the bits of `other`.
    pub const fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Clear the bits of `other`.
    pub const fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl BitOr for MouseButtons {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl BitAnd for MouseButtons {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

/// Keyboard/Keypad page (0x07) usages.
pub mod keyboard {
    pub const A: u8 = 0x04;
    pub const B: u8 = 0x05;
    pub const C: u8 = 0x06;
    pub const D: u8 = 0x07;
    pub const E: u8 = 0x08;
    pub const F: u8 = 0x09;
    pub const G: u8 = 0x0A;
    pub const H: u8 = 0x0B;
    pub const I: u8 = 0x0C;
    pub const J: u8 = 0x0D;
    pub const K: u8 = 0x0E;
    pub const L: u8 = 0x0F;
    pub const M: u8 = 0x10;
    pub const N: u8 = 0x11;
    pub const O: u8 = 0x12;
    pub const P: u8 = 0x13;
    pub const Q: u8 = 0x14;
    pub const R: u8 = 0x15;
    pub const S: u8 = 0x16;
    pub const T: u8 = 0x17;
    pub const U: u8 = 0x18;
    pub const V: u8 = 0x19;
    pub const W: u8 = 0x1A;
    pub const X: u8 = 0x1B;
    pub const Y: u8 = 0x1C;
    pub const Z: u8 = 0x1D;

    pub const NUMBER_1: u8 = 0x1E;
    pub const NUMBER_2: u8 = 0x1F;
    pub const NUMBER_3: u8 = 0x20;
    pub const NUMBER_4: u8 = 0x21;
    pub const NUMBER_5: u8 = 0x22;
    pub const NUMBER_6: u8 = 0x23;
    pub const NUMBER_7: u8 = 0x24;
    pub const NUMBER_8: u8 = 0x25;
    pub const NUMBER_9: u8 = 0x26;
    pub const NUMBER_0: u8 = 0x27;

    pub const ENTER: u8 = 0x28;
    pub const ESCAPE: u8 = 0x29;
    pub const BACKSPACE: u8 = 0x2A;
    pub const TAB: u8 = 0x2B;
    pub const SPACE: u8 = 0x2C;

    pub const CAPS_LOCK: u8 = 0x39;

    pub const F1: u8 = 0x3A;
    pub const F2: u8 = 0x3B;
    pub const F3: u8 = 0x3C;
    pub const F4: u8 = 0x3D;
    pub const F5: u8 = 0x3E;
    pub const F6: u8 = 0x3F;
    pub const F7: u8 = 0x40;
    pub const F8: u8 = 0x41;
    pub const F9: u8 = 0x42;
    pub const F10: u8 = 0x43;
    pub const F11: u8 = 0x44;
    pub const F12: u8 = 0x45;

    pub const PRINT_SCREEN: u8 = 0x46;
    pub const SCROLL_LOCK: u8 = 0x47;
    pub const PAUSE: u8 = 0x48;

    pub const INSERT: u8 = 0x49;
    pub const HOME: u8 = 0x4A;
    pub const PAGE_UP: u8 = 0x4B;
    pub const DELETE: u8 = 0x4C;
    pub const END: u8 = 0x4D;
    pub const PAGE_DOWN: u8 = 0x4E;

    pub const ARROW_RIGHT: u8 = 0x4F;
    pub const ARROW_LEFT: u8 = 0x50;
    pub const ARROW_DOWN: u8 = 0x51;
    pub const ARROW_UP: u8 = 0x52;

    pub const F13: u8 = 0x68;
    pub const F14: u8 = 0x69;
    pub const F15: u8 = 0x6A;
    pub const F16: u8 = 0x6B;
    pub const F17: u8 = 0x6C;
    pub const F18: u8 = 0x6D;
    pub const F19: u8 = 0x6E;
    pub const F20: u8 = 0x6F;
    pub const F21: u8 = 0x70;
    pub const F22: u8 = 0x71;
    pub const F23: u8 = 0x72;
    pub const F24: u8 = 0x73;
}

/// Consumer page (0x0C) usages: media remote controls.
pub mod consumer {
    pub const PLAY_PAUSE: u16 = 0x00CD;
    pub const NEXT_TRACK: u16 = 0x00B5;
    pub const PREVIOUS_TRACK: u16 = 0x00B6;
    pub const STOP: u16 = 0x00B7;
    pub const VOLUME_UP: u16 = 0x00E9;
    pub const VOLUME_DOWN: u16 = 0x00EA;
    pub const MUTE: u16 = 0x00E2;
}

/// Generic Desktop page (0x01) system control usages.
pub mod system {
    /// System power down.
    pub const POWER_DOWN: u8 = 0x81;
    /// System sleep.
    pub const SLEEP: u8 = 0x82;
    /// System wake up.
    pub const WAKE_UP: u8 = 0x83;
}

/// Carries only the ABI version and a reserved field. Input of [`GET_STATUS`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionAbi {
    pub version: u16,
    pub reserved: u16,
}

/// Connection state and keyboard indicators of the computer. Output of [`GET_STATUS`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusAbi {
    pub version: u16,
    /// `1` when a computer is connected and listening, else `0`.
    pub connected: u8,
    /// Keyboard indicator bitmask from the computer: [`INDICATOR_NUM_LOCK`],
    /// [`INDICATOR_CAPS_LOCK`], [`INDICATOR_SCROLL_LOCK`].
    pub indicators: u8,
    pub reserved: [u8; 4],
}

define_command!(
    GET_STATUS,
    Read,
    crate::kind::HUMAN_INTERFACE,
    0x00,
    VersionAbi,
    StatusAbi
);

#[cfg(test)]
mod tests {
    use super::*;
    use file_system::ControlDirectionFlags;

    fn keyboard_message() -> Message {
        Message::Keyboard {
            modifiers: Modifiers::LEFT_CONTROL.union(Modifiers::LEFT_SHIFT),
            keys: [keyboard::A, keyboard::ENTER, 0, 0, 0, 0],
        }
    }

    #[test]
    fn encodes_and_decodes_every_kind() {
        let messages = [
            keyboard_message(),
            Message::Keyboard {
                modifiers: Modifiers::empty(),
                keys: [0; KEY_COUNT],
            },
            Message::Consumer {
                usage: consumer::PLAY_PAUSE,
            },
            Message::Consumer { usage: 0 },
            Message::Mouse {
                buttons: MouseButtons::LEFT.union(MouseButtons::RIGHT),
                x: -12,
                y: 34,
                wheel: -1,
                pan: 2,
            },
            Message::Mouse {
                buttons: MouseButtons::empty(),
                x: 0,
                y: 0,
                wheel: 0,
                pan: 0,
            },
            Message::System {
                usage: system::POWER_DOWN,
            },
            Message::System { usage: 0 },
        ];
        for message in messages {
            let encoded = message.encode();
            assert_eq!(encoded.len(), MESSAGE_SIZE);
            assert_eq!(Message::decode(&encoded), Ok(message));
        }
    }

    #[test]
    fn encodes_the_documented_wire_layout() {
        assert_eq!(
            keyboard_message().encode(),
            [
                KIND_KEYBOARD,
                0b0000_0011,
                keyboard::A,
                keyboard::ENTER,
                0,
                0,
                0,
                0
            ]
        );
        assert_eq!(
            Message::Consumer {
                usage: consumer::NEXT_TRACK
            }
            .encode(),
            [KIND_CONSUMER, 0xB5, 0x00, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            Message::Mouse {
                buttons: MouseButtons::MIDDLE,
                x: -1,
                y: 127,
                wheel: -128,
                pan: 5,
            }
            .encode(),
            [KIND_MOUSE, 0b0000_0100, 0xFF, 0x7F, 0x80, 0x05, 0, 0]
        );
        assert_eq!(
            Message::System {
                usage: system::WAKE_UP
            }
            .encode(),
            [KIND_SYSTEM, 0x83, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn decode_rejects_an_unknown_kind() {
        for kind in [0u8, 5, 42, u8::MAX] {
            let mut bytes = [0u8; MESSAGE_SIZE];
            bytes[0] = kind;
            assert_eq!(Message::decode(&bytes), Err(Error::InvalidInput));
        }
    }

    #[test]
    fn decode_ignores_reserved_bytes() {
        let mut bytes = Message::Consumer { usage: 0x1234 }.encode();
        bytes[3] = 0xAA;
        bytes[7] = 0xBB;
        assert_eq!(
            Message::decode(&bytes),
            Ok(Message::Consumer { usage: 0x1234 })
        );
    }

    #[test]
    fn messages_iterator_checks_the_write_length() {
        assert_eq!(Messages::new(&[]).unwrap().count(), 0);
        let bytes = [0u8; 80];
        for length in [1, 7, 9, 15, 17] {
            assert_eq!(
                Messages::new(&bytes[..length]).unwrap_err(),
                Error::InvalidInput
            );
        }
        for length in [0, 8, 16, 80] {
            assert!(Messages::new(&bytes[..length]).is_ok());
        }
    }

    #[test]
    fn messages_iterator_decodes_each_message_of_a_write() {
        let mut write = [0u8; 3 * MESSAGE_SIZE];
        write[..MESSAGE_SIZE].copy_from_slice(&keyboard_message().encode());
        write[MESSAGE_SIZE..2 * MESSAGE_SIZE]
            .copy_from_slice(&Message::Consumer { usage: 0 }.encode());
        write[2 * MESSAGE_SIZE..].copy_from_slice(&[9u8; MESSAGE_SIZE]);
        let mut messages = Messages::new(&write).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages.next(), Some(Ok(keyboard_message())));
        assert_eq!(messages.next(), Some(Ok(Message::Consumer { usage: 0 })));
        assert_eq!(messages.next(), Some(Err(Error::InvalidInput)));
        assert_eq!(messages.next(), None);
    }

    #[test]
    fn modifiers_use_the_hid_standard_bit_order() {
        assert_eq!(Modifiers::LEFT_CONTROL.bits(), 0x01);
        assert_eq!(Modifiers::LEFT_SHIFT.bits(), 0x02);
        assert_eq!(Modifiers::LEFT_ALT.bits(), 0x04);
        assert_eq!(Modifiers::LEFT_GUI.bits(), 0x08);
        assert_eq!(Modifiers::RIGHT_CONTROL.bits(), 0x10);
        assert_eq!(Modifiers::RIGHT_SHIFT.bits(), 0x20);
        assert_eq!(Modifiers::RIGHT_ALT.bits(), 0x40);
        assert_eq!(Modifiers::RIGHT_GUI.bits(), 0x80);
        assert_eq!(Modifiers::ALL.bits(), 0xFF);

        let shortcut = Modifiers::LEFT_CONTROL | Modifiers::LEFT_SHIFT;
        assert!(shortcut.contains(Modifiers::LEFT_CONTROL));
        assert!(shortcut.contains(Modifiers::LEFT_SHIFT));
        assert!(!shortcut.contains(Modifiers::LEFT_ALT));
        assert!(shortcut.intersects(Modifiers::LEFT_SHIFT));
        assert!(!shortcut.intersects(Modifiers::RIGHT_CONTROL));
        let mut pressed = Modifiers::empty();
        assert!(pressed.is_empty());
        pressed.insert(Modifiers::RIGHT_GUI);
        assert_eq!(pressed, Modifiers::RIGHT_GUI);
        pressed.remove(Modifiers::RIGHT_GUI);
        assert!(pressed.is_empty());
        assert_eq!(
            (Modifiers::LEFT_ALT & Modifiers::ALL).bits(),
            Modifiers::LEFT_ALT.bits()
        );
    }

    #[test]
    fn mouse_buttons_use_left_right_middle_order() {
        assert_eq!(MouseButtons::LEFT.bits(), 0x01);
        assert_eq!(MouseButtons::RIGHT.bits(), 0x02);
        assert_eq!(MouseButtons::MIDDLE.bits(), 0x04);
        assert_eq!(MouseButtons::ALL.bits(), 0x07);
        let both = MouseButtons::LEFT | MouseButtons::MIDDLE;
        assert!(both.contains(MouseButtons::LEFT));
        assert!(!both.contains(MouseButtons::RIGHT));
        assert!(MouseButtons::empty().is_empty());
    }

    #[test]
    fn usage_constants_match_the_hid_pages() {
        assert_eq!(keyboard::A, 0x04);
        assert_eq!(keyboard::Z, 0x1D);
        assert_eq!(keyboard::NUMBER_1, 0x1E);
        assert_eq!(keyboard::NUMBER_0, 0x27);
        assert_eq!(keyboard::ENTER, 0x28);
        assert_eq!(keyboard::ESCAPE, 0x29);
        assert_eq!(keyboard::TAB, 0x2B);
        assert_eq!(keyboard::SPACE, 0x2C);
        assert_eq!(keyboard::F1, 0x3A);
        assert_eq!(keyboard::F12, 0x45);
        assert_eq!(keyboard::PRINT_SCREEN, 0x46);
        assert_eq!(keyboard::INSERT, 0x49);
        assert_eq!(keyboard::HOME, 0x4A);
        assert_eq!(keyboard::PAGE_UP, 0x4B);
        assert_eq!(keyboard::DELETE, 0x4C);
        assert_eq!(keyboard::END, 0x4D);
        assert_eq!(keyboard::PAGE_DOWN, 0x4E);
        assert_eq!(keyboard::ARROW_RIGHT, 0x4F);
        assert_eq!(keyboard::ARROW_LEFT, 0x50);
        assert_eq!(keyboard::ARROW_DOWN, 0x51);
        assert_eq!(keyboard::ARROW_UP, 0x52);
        assert_eq!(keyboard::F13, 0x68);
        assert_eq!(keyboard::F24, 0x73);

        assert_eq!(consumer::PLAY_PAUSE, 0x00CD);
        assert_eq!(consumer::NEXT_TRACK, 0x00B5);
        assert_eq!(consumer::PREVIOUS_TRACK, 0x00B6);
        assert_eq!(consumer::STOP, 0x00B7);
        assert_eq!(consumer::VOLUME_UP, 0x00E9);
        assert_eq!(consumer::VOLUME_DOWN, 0x00EA);
        assert_eq!(consumer::MUTE, 0x00E2);

        assert_eq!(system::POWER_DOWN, 0x81);
        assert_eq!(system::SLEEP, 0x82);
        assert_eq!(system::WAKE_UP, 0x83);

        assert_eq!(INDICATOR_NUM_LOCK, 0x01);
        assert_eq!(INDICATOR_CAPS_LOCK, 0x02);
        assert_eq!(INDICATOR_SCROLL_LOCK, 0x04);
    }

    #[test]
    fn status_command_uses_the_human_interface_kind_and_is_readable() {
        let identifier = GET_STATUS::IDENTIFIER;
        assert_eq!(identifier.get_kind(), crate::kind::HUMAN_INTERFACE);
        assert_eq!(usize::from(identifier.get_number()), 0);
        assert!(
            identifier
                .get_direction()
                .contains(ControlDirectionFlags::Read)
        );
        assert!(
            !identifier
                .get_direction()
                .contains(ControlDirectionFlags::Write)
        );
    }

    #[test]
    fn abi_structs_have_the_documented_layout() {
        assert_eq!(size_of::<VersionAbi>(), 4);
        assert_eq!(size_of::<StatusAbi>(), 8);
        assert_eq!(
            StatusAbi {
                version: ABI_VERSION,
                connected: 1,
                indicators: INDICATOR_NUM_LOCK | INDICATOR_CAPS_LOCK,
                reserved: [0; 4],
            },
            StatusAbi {
                version: 1,
                connected: 1,
                indicators: 0b011,
                reserved: [0; 4],
            }
        );
    }
}
