//! Character device contracts, including the terminal line discipline controls.
//!
//! # Terminal control commands (kind `b'T'`)
//!
//! | Number | Command                   | Direction | Input                  | Output                 | Effect                                         |
//! |--------|---------------------------|-----------|------------------------|------------------------|------------------------------------------------|
//! | `0x00` | `IS_A_TERMINAL`           | Read      | `()`                   | `bool`                 | Whether the device is an interactive terminal. |
//! | `0x01` | `GET_TERMINAL_ATTRIBUTES` | Read      | `()`                   | [`TerminalAttributes`] | Returns the current line discipline settings.  |
//! | `0x02` | `SET_TERMINAL_ATTRIBUTES` | Read      | [`TerminalAttributes`] | `()`                   | Applies new line discipline settings.          |
//!
//! These commands play the role of POSIX `tcgetattr` and `tcsetattr`.
//! `SET_TERMINAL_ATTRIBUTES` is declared with the `Read` direction on purpose:
//! a program changes the settings of its standard input, which is opened
//! read-only, just as `tcsetattr` accepts a read-only descriptor. The VFS
//! requires write access only for commands declared `Write`.
//!
//! A device that has no line discipline answers both commands with
//! `Unsupported`; callers then keep their usual behaviour. A device
//! rejects flags it does not know with `InvalidArgument`, so a program can
//! tell when a setting it asked for is not available.

use crate::{
    BaseOperations, ControlCommand, DirectBaseOperations, MountOperations, define_command,
};
use bitflags::bitflags;

// Kind byte `b'T'` is registered as `device::kind::TERMINAL` (kept as a literal here because `device` depends on `file_system`).
define_command!(IS_A_TERMINAL, Read, b'T', 0, (), bool);
define_command!(
    GET_TERMINAL_ATTRIBUTES,
    Read,
    b'T',
    1,
    (),
    TerminalAttributes
);
define_command!(
    SET_TERMINAL_ATTRIBUTES,
    Read,
    b'T',
    2,
    TerminalAttributes,
    ()
);

bitflags! {
    /// Line discipline switches of a terminal, see [`TerminalAttributes`].
    ///
    /// Unused bits are reserved for future switches.
    #[repr(transparent)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct TerminalFlags: u32 {
        /// Write typed characters back to the terminal (POSIX `ECHO`).
        ///
        /// When clear in canonical mode, the device still writes the line
        /// break that ends a line, so the cursor leaves the prompt line
        /// (POSIX `ECHO` clear with `ECHONL` set).
        const Echo = 1 << 0;
        /// Deliver input line by line, with line editing (POSIX `ICANON`).
        ///
        /// When clear, the device delivers bytes as they arrive, without any
        /// editing or translation.
        const Canonical = 1 << 1;
    }
}

/// Terminal line discipline settings, exchanged by
/// [`GET_TERMINAL_ATTRIBUTES`] and [`SET_TERMINAL_ATTRIBUTES`].
///
/// To change one setting, read the current attributes, update them and
/// write them back, as with `tcgetattr` and `tcsetattr`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalAttributes {
    pub flags: TerminalFlags,
}

impl TerminalAttributes {
    /// Canonical input with echo: the usual interactive terminal.
    pub const INTERACTIVE: Self = Self {
        flags: TerminalFlags::Echo.union(TerminalFlags::Canonical),
    };

    /// Raw input: bytes are delivered as they arrive, without echo.
    pub const RAW: Self = Self {
        flags: TerminalFlags::empty(),
    };

    pub const fn new(flags: TerminalFlags) -> Self {
        Self { flags }
    }

    pub const fn is_echo_enabled(&self) -> bool {
        self.flags.contains(TerminalFlags::Echo)
    }

    pub const fn is_canonical(&self) -> bool {
        self.flags.contains(TerminalFlags::Canonical)
    }

    /// Returns a copy with echo turned on or off.
    pub const fn with_echo(self, enabled: bool) -> Self {
        self.with_flag(TerminalFlags::Echo, enabled)
    }

    /// Returns a copy with canonical input turned on or off.
    pub const fn with_canonical(self, enabled: bool) -> Self {
        self.with_flag(TerminalFlags::Canonical, enabled)
    }

    /// Whether every set flag is one defined by this version of the contract.
    pub const fn has_only_known_flags(&self) -> bool {
        TerminalFlags::all().contains(self.flags)
    }

    const fn with_flag(self, flag: TerminalFlags, enabled: bool) -> Self {
        Self {
            flags: if enabled {
                self.flags.union(flag)
            } else {
                self.flags.difference(flag)
            },
        }
    }
}

impl Default for TerminalAttributes {
    fn default() -> Self {
        Self::INTERACTIVE
    }
}

pub trait CharacterDevice: BaseOperations + MountOperations {
    /// Snapshot device readiness without consuming data. Return `None` to use
    /// the VFS fallback, which reports the requested read/write directions
    /// allowed by the opening's access mode. I/O on such devices may still suspend.
    ///
    /// Drivers overriding this with a precise probe must call
    /// [`crate::notify_readiness`] after changes that can wake a poller.
    fn readiness(
        &self,
        _interests: crate::ReadyEvents,
    ) -> crate::Result<Option<crate::ReadyEvents>> {
        Ok(None)
    }
}

pub trait DirectCharacterDevice: DirectBaseOperations + MountOperations {
    /// See [`CharacterDevice::readiness`] for the default and notification contract.
    fn readiness(
        &self,
        _interests: crate::ReadyEvents,
    ) -> crate::Result<Option<crate::ReadyEvents>> {
        Ok(None)
    }
}

impl<T: DirectCharacterDevice> CharacterDevice for T {
    fn readiness(
        &self,
        interests: crate::ReadyEvents,
    ) -> crate::Result<Option<crate::ReadyEvents>> {
        DirectCharacterDevice::readiness(self, interests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ControlDirectionFlags;

    #[test]
    fn terminal_identifiers_are_distinct_and_share_the_terminal_kind() {
        let identifiers = [
            IS_A_TERMINAL::IDENTIFIER,
            GET_TERMINAL_ATTRIBUTES::IDENTIFIER,
            SET_TERMINAL_ATTRIBUTES::IDENTIFIER,
        ];
        for (index, left) in identifiers.iter().enumerate() {
            assert_eq!(left.get_kind(), b'T');
            assert_eq!(usize::from(left.get_number()), index);
            for right in &identifiers[index + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    #[test]
    fn terminal_attribute_commands_work_on_read_only_handles() {
        for identifier in [
            GET_TERMINAL_ATTRIBUTES::IDENTIFIER,
            SET_TERMINAL_ATTRIBUTES::IDENTIFIER,
        ] {
            assert!(
                !identifier
                    .get_direction()
                    .contains(ControlDirectionFlags::Write)
            );
        }
    }

    #[test]
    fn terminal_attributes_layout_is_one_32_bit_word() {
        assert_eq!(size_of::<TerminalAttributes>(), 4);
        assert_eq!(align_of::<TerminalAttributes>(), 4);
        assert_eq!(TerminalFlags::Echo.bits(), 1 << 0);
        assert_eq!(TerminalFlags::Canonical.bits(), 1 << 1);
    }

    #[test]
    fn terminal_attributes_default_to_interactive_input() {
        let attributes = TerminalAttributes::default();
        assert_eq!(attributes, TerminalAttributes::INTERACTIVE);
        assert!(attributes.is_echo_enabled());
        assert!(attributes.is_canonical());

        assert!(!TerminalAttributes::RAW.is_echo_enabled());
        assert!(!TerminalAttributes::RAW.is_canonical());
    }

    #[test]
    fn terminal_attributes_toggle_one_flag_at_a_time() {
        let without_echo = TerminalAttributes::INTERACTIVE.with_echo(false);
        assert!(!without_echo.is_echo_enabled());
        assert!(without_echo.is_canonical());
        assert_eq!(
            without_echo.with_echo(true),
            TerminalAttributes::INTERACTIVE
        );

        let raw_with_echo = TerminalAttributes::INTERACTIVE.with_canonical(false);
        assert!(raw_with_echo.is_echo_enabled());
        assert!(!raw_with_echo.is_canonical());
        assert_eq!(raw_with_echo.with_echo(false), TerminalAttributes::RAW);
    }

    #[test]
    fn terminal_attributes_detect_unknown_flags() {
        assert!(TerminalAttributes::INTERACTIVE.has_only_known_flags());
        assert!(TerminalAttributes::RAW.has_only_known_flags());

        let future = TerminalAttributes::new(TerminalFlags::from_bits_retain(1 << 31));
        assert!(!future.has_only_known_flags());
        // Toggling a known flag keeps reserved bits untouched.
        assert_eq!(future.with_echo(true).flags.bits(), (1 << 31) | 1);
    }
}
