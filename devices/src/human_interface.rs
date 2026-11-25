//! Human interface emulation device contract (`/devices/human_interface/emulation`, kind `b'h'`).
//!
//! A Xila device can act as a USB or Bluetooth keyboard, media remote and mouse toward the
//! computer it is connected to (shortcuts, screenshots, play/pause, volume). An application
//! writes messages to the device node. The upper half
//! (`drivers_peripherals::human_interface`) validates the message structure and forwards
//! HID reports to the target controller (`peripherals::human_interface::HumanInterfaceController`),
//! which every USB and Bluetooth Low Energy lower half implements. The shared HID report
//! descriptor lives in `peripherals::human_interface`.
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
//! whose length is not a multiple of 8 fails with `InvalidArgument`. Every message must
//! decode: an unknown kind in byte 0 or a non-zero reserved byte makes the message invalid.
//! Reserved bytes are written as zero on encode.
//!
//! The upper half sends the messages of one write one by one and returns the number of
//! accepted bytes: when the first message is invalid or its report cannot be sent, the
//! write fails with an error; when a later message fails, the write returns the accepted
//! byte count, a multiple of [`MESSAGE_SIZE`].
//!
//! Byte 0 of a message is the kind:
//!
//! | Kind | Name             | Byte 1                                             | Bytes 2 to 7                                                                |
//! |------|------------------|----------------------------------------------------|-----------------------------------------------------------------------------|
//! | `1`  | keyboard         | modifier bitmask, see [`Modifiers`]               | up to six key usages from the HID Keyboard/Keypad page (0x07), `0` = none   |
//! | `2`  | consumer control | consumer usage (HID page 0x0C), little endian, `0` = release | must be `0`                                                          |
//! | `3`  | mouse            | buttons bitmask, see [`MouseButtons`]              | x (i8), y (i8), wheel (i8), pan (i8) in bytes 2 to 5, bytes 6 and 7 must be `0` |
//! | `4`  | system control   | usage from the HID Generic Desktop page: `0x81` power down, `0x82` sleep, `0x83` wake up, `0` = release | must be `0` |
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
    /// [`MESSAGE_SIZE`], or a non-zero reserved byte.
    InvalidArgument,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument => write!(formatter, "invalid human interface message"),
        }
    }
}

impl core::error::Error for Error {}

/// Generates a hand-written bitmask type with the given named bits.
macro_rules! bitmask {
    (
        $(#[$type_doc:meta])*
        $name:ident, $bits:ty;
        $( $(#[$doc:meta])* $constant:ident = $value:expr; )*
    ) => {
        $(#[$type_doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name($bits);

        impl $name {
            /// No bit set.
            pub const NONE: Self = Self(0);
            $( $(#[$doc])* pub const $constant: Self = Self($value); )*

            /// Every defined bit.
            pub const ALL: Self = Self(0 $(| $value)*);

            /// Empty bitmask.
            pub const fn empty() -> Self {
                Self::NONE
            }

            /// Bitmask from raw bits; unknown bits are retained.
            pub const fn from_bits_retain(bits: $bits) -> Self {
                Self(bits)
            }

            /// Raw bits of the bitmask.
            pub const fn bits(self) -> $bits {
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

        impl BitOr for $name {
            type Output = Self;
            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl BitAnd for $name {
            type Output = Self;
            fn bitand(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }
        }
    };
}

bitmask! {
    /// Keyboard modifier bitmask, HID standard order: left control, left shift, left alt,
    /// left GUI, then the right ones.
    Modifiers, u8;
    /// Left control.
    LEFT_CONTROL = 1 << 0;
    /// Left shift.
    LEFT_SHIFT = 1 << 1;
    /// Left alt.
    LEFT_ALT = 1 << 2;
    /// Left GUI (Windows, command, super).
    LEFT_GUI = 1 << 3;
    /// Right control.
    RIGHT_CONTROL = 1 << 4;
    /// Right shift.
    RIGHT_SHIFT = 1 << 5;
    /// Right alt (alt gr).
    RIGHT_ALT = 1 << 6;
    /// Right GUI.
    RIGHT_GUI = 1 << 7;
}

bitmask! {
    /// Mouse buttons bitmask: left, right, middle.
    MouseButtons, u8;
    /// Left button.
    LEFT = 1 << 0;
    /// Right button.
    RIGHT = 1 << 1;
    /// Middle button.
    MIDDLE = 1 << 2;
}

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
    /// Encode the message as its 8 wire bytes, kind in byte 0. Reserved bytes are zero.
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

    /// Decode one 8-byte message. An unknown kind in byte 0 or a non-zero reserved byte
    /// fails with [`Error::InvalidArgument`].
    pub fn decode(bytes: &[u8; MESSAGE_SIZE]) -> Result<Self, Error> {
        /// Reserved bytes of a message must be zero.
        fn check_reserved(reserved: &[u8]) -> Result<(), Error> {
            if reserved.iter().any(|&byte| byte != 0) {
                return Err(Error::InvalidArgument);
            }
            Ok(())
        }
        match bytes[0] {
            KIND_KEYBOARD => Ok(Self::Keyboard {
                modifiers: Modifiers::from_bits_retain(bytes[1]),
                keys: bytes[2..]
                    .try_into()
                    .expect("slice of MESSAGE_SIZE - 2 bytes"),
            }),
            KIND_CONSUMER => {
                check_reserved(&bytes[3..])?;
                Ok(Self::Consumer {
                    usage: u16::from_le_bytes([bytes[1], bytes[2]]),
                })
            }
            KIND_MOUSE => {
                check_reserved(&bytes[6..])?;
                Ok(Self::Mouse {
                    buttons: MouseButtons::from_bits_retain(bytes[1]),
                    x: bytes[2] as i8,
                    y: bytes[3] as i8,
                    wheel: bytes[4] as i8,
                    pan: bytes[5] as i8,
                })
            }
            KIND_SYSTEM => {
                check_reserved(&bytes[2..])?;
                Ok(Self::System { usage: bytes[1] })
            }
            _ => Err(Error::InvalidArgument),
        }
    }
}

/// Iterator over the messages of one write.
///
/// [`Messages::new`] checks that the length of the write is a multiple of [`MESSAGE_SIZE`];
/// every item decodes one message and fails with [`Error::InvalidArgument`] on an unknown kind
/// or a non-zero reserved byte.
#[derive(Clone, Debug)]
pub struct Messages<'a> {
    remaining: &'a [u8],
}

impl<'a> Messages<'a> {
    /// Validate the write length and return its message iterator.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        if !bytes.len().is_multiple_of(MESSAGE_SIZE) {
            return Err(Error::InvalidArgument);
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

/// Generates the Keyboard/Keypad page (0x07) usage constants with doc comments.
macro_rules! keyboard_usages {
    ( $( $name:ident = $value:expr, $doc:literal; )* ) => {
        /// Keyboard/Keypad page (0x07) usages.
        pub mod keyboard {
            $( #[doc = $doc] pub const $name: u8 = $value; )*
        }
    };
}

keyboard_usages! {
    A = 0x04, "Letter A key.";
    B = 0x05, "Letter B key.";
    C = 0x06, "Letter C key.";
    D = 0x07, "Letter D key.";
    E = 0x08, "Letter E key.";
    F = 0x09, "Letter F key.";
    G = 0x0A, "Letter G key.";
    H = 0x0B, "Letter H key.";
    I = 0x0C, "Letter I key.";
    J = 0x0D, "Letter J key.";
    K = 0x0E, "Letter K key.";
    L = 0x0F, "Letter L key.";
    M = 0x10, "Letter M key.";
    N = 0x11, "Letter N key.";
    O = 0x12, "Letter O key.";
    P = 0x13, "Letter P key.";
    Q = 0x14, "Letter Q key.";
    R = 0x15, "Letter R key.";
    S = 0x16, "Letter S key.";
    T = 0x17, "Letter T key.";
    U = 0x18, "Letter U key.";
    V = 0x19, "Letter V key.";
    W = 0x1A, "Letter W key.";
    X = 0x1B, "Letter X key.";
    Y = 0x1C, "Letter Y key.";
    Z = 0x1D, "Letter Z key.";
    NUMBER_1 = 0x1E, "Digit 1 key.";
    NUMBER_2 = 0x1F, "Digit 2 key.";
    NUMBER_3 = 0x20, "Digit 3 key.";
    NUMBER_4 = 0x21, "Digit 4 key.";
    NUMBER_5 = 0x22, "Digit 5 key.";
    NUMBER_6 = 0x23, "Digit 6 key.";
    NUMBER_7 = 0x24, "Digit 7 key.";
    NUMBER_8 = 0x25, "Digit 8 key.";
    NUMBER_9 = 0x26, "Digit 9 key.";
    NUMBER_0 = 0x27, "Digit 0 key.";
    ENTER = 0x28, "Enter (return) key.";
    ESCAPE = 0x29, "Escape key.";
    BACKSPACE = 0x2A, "Backspace key.";
    TAB = 0x2B, "Tab key.";
    SPACE = 0x2C, "Space bar.";
    CAPS_LOCK = 0x39, "Caps lock key.";
    F1 = 0x3A, "Function key F1.";
    F2 = 0x3B, "Function key F2.";
    F3 = 0x3C, "Function key F3.";
    F4 = 0x3D, "Function key F4.";
    F5 = 0x3E, "Function key F5.";
    F6 = 0x3F, "Function key F6.";
    F7 = 0x40, "Function key F7.";
    F8 = 0x41, "Function key F8.";
    F9 = 0x42, "Function key F9.";
    F10 = 0x43, "Function key F10.";
    F11 = 0x44, "Function key F11.";
    F12 = 0x45, "Function key F12.";
    PRINT_SCREEN = 0x46, "Print screen key.";
    SCROLL_LOCK = 0x47, "Scroll lock key.";
    PAUSE = 0x48, "Pause (break) key.";
    INSERT = 0x49, "Insert key.";
    HOME = 0x4A, "Home key.";
    PAGE_UP = 0x4B, "Page up key.";
    DELETE = 0x4C, "Delete (forward) key.";
    END = 0x4D, "End key.";
    PAGE_DOWN = 0x4E, "Page down key.";
    ARROW_RIGHT = 0x4F, "Right arrow key.";
    ARROW_LEFT = 0x50, "Left arrow key.";
    ARROW_DOWN = 0x51, "Down arrow key.";
    ARROW_UP = 0x52, "Up arrow key.";
    F13 = 0x68, "Function key F13.";
    F14 = 0x69, "Function key F14.";
    F15 = 0x6A, "Function key F15.";
    F16 = 0x6B, "Function key F16.";
    F17 = 0x6C, "Function key F17.";
    F18 = 0x6D, "Function key F18.";
    F19 = 0x6E, "Function key F19.";
    F20 = 0x6F, "Function key F20.";
    F21 = 0x70, "Function key F21.";
    F22 = 0x71, "Function key F22.";
    F23 = 0x72, "Function key F23.";
    F24 = 0x73, "Function key F24.";
}

/// Consumer page (0x0C) usages: media remote controls.
pub mod consumer {
    /// Play/pause.
    pub const PLAY_PAUSE: u16 = 0x00CD;
    /// Next track.
    pub const NEXT_TRACK: u16 = 0x00B5;
    /// Previous track.
    pub const PREVIOUS_TRACK: u16 = 0x00B6;
    /// Stop.
    pub const STOP: u16 = 0x00B7;
    /// Volume up.
    pub const VOLUME_UP: u16 = 0x00E9;
    /// Volume down.
    pub const VOLUME_DOWN: u16 = 0x00EA;
    /// Mute.
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

file_system::plain_data! {
    #[repr(C)]
    /// Carries only the ABI version and a reserved field. Input of [`GET_STATUS`].
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct VersionAbi {
        pub version: u16,
        pub reserved: u16,
    }
}

file_system::plain_data! {
    #[repr(C)]
    /// Connection state and keyboard indicators of the computer. Output of [`GET_STATUS`].
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
            assert_eq!(Message::decode(&bytes), Err(Error::InvalidArgument));
        }
    }

    #[test]
    fn decode_rejects_non_zero_reserved_bytes() {
        let mut consumer = Message::Consumer { usage: 0x1234 }.encode();
        consumer[3] = 0xAA;
        assert_eq!(Message::decode(&consumer), Err(Error::InvalidArgument));

        let mut mouse = Message::Mouse {
            buttons: MouseButtons::empty(),
            x: 0,
            y: 0,
            wheel: 0,
            pan: 0,
        }
        .encode();
        mouse[6] = 0x01;
        assert_eq!(Message::decode(&mouse), Err(Error::InvalidArgument));

        let mut system = Message::System { usage: 0x81 }.encode();
        system[7] = 0xFF;
        assert_eq!(Message::decode(&system), Err(Error::InvalidArgument));

        // The keyboard message uses every byte, so nothing is reserved there.
        let mut keyboard = keyboard_message().encode();
        keyboard[7] = 0xFF;
        assert!(Message::decode(&keyboard).is_ok());
    }

    #[test]
    fn messages_iterator_checks_the_write_length() {
        assert_eq!(Messages::new(&[]).unwrap().count(), 0);
        let bytes = [0u8; 80];
        for length in [1, 7, 9, 15, 17] {
            assert_eq!(
                Messages::new(&bytes[..length]).unwrap_err(),
                Error::InvalidArgument
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
        assert_eq!(messages.next(), Some(Err(Error::InvalidArgument)));
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
