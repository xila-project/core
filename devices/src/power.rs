//! System power device contract (`/devices/power`, kind `b'P'`).
//!
//! The device performs hardware power actions only. It does not coordinate an
//! OS shutdown: the lifecycle subsystem stops tasks and commits storage first,
//! then calls this device as its final step. Hosts restrict the node to
//! privileged users through VFS permissions.
//!
//! | Number | Command            | Direction | Input | Output | Effect                                   |
//! |--------|--------------------|-----------|-------|--------|------------------------------------------|
//! | `0x00` | `GET_CAPABILITIES` | Read      | `()`  | `u8`   | Bit set of [`Capabilities`].             |
//! | `0x01` | `POWER_OFF`        | Write     | `u8`  | `()`   | Halt or power off; input: exit status.   |
//! | `0x02` | `RESTART`          | Write     | `u8`  | `()`   | Reset or restart; input: exit status.    |
//! | `0x03` | `SLEEP`            | Write     | `()`  | `()`   | Enter a low-power state it resumes from. |
//!
//! The exit status is meaningful only to hosted targets (process exit code).
//! An unsupported action fails with `UnsupportedOperation`; a successful
//! action may never return. Payloads are plain bytes.

use file_system::{ControlCommand, define_command};

define_command!(GET_CAPABILITIES, Read, crate::kind::POWER, 0, (), u8);
define_command!(POWER_OFF, Write, crate::kind::POWER, 1, u8, ());
define_command!(RESTART, Write, crate::kind::POWER, 2, u8, ());
define_command!(SLEEP, Write, crate::kind::POWER, 3, (), ());

/// Bit set of power actions a target really performs.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities(pub u8);

impl Capabilities {
    pub const NONE: Self = Self(0);
    pub const POWER_OFF: Self = Self(1 << 0);
    pub const RESTART: Self = Self(1 << 1);
    pub const SLEEP: Self = Self(1 << 2);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use file_system::ControlDirectionFlags;

    #[test]
    fn identifiers_are_distinct_and_actions_need_write_access() {
        let identifiers = [
            GET_CAPABILITIES::IDENTIFIER,
            POWER_OFF::IDENTIFIER,
            RESTART::IDENTIFIER,
            SLEEP::IDENTIFIER,
        ];
        for (index, left) in identifiers.iter().enumerate() {
            for right in &identifiers[index + 1..] {
                assert_ne!(left, right);
            }
        }
        for action in &identifiers[1..] {
            assert!(
                action
                    .get_direction()
                    .contains(ControlDirectionFlags::Write)
            );
        }
    }

    #[test]
    fn capabilities_combine() {
        let capabilities = Capabilities::POWER_OFF.union(Capabilities::RESTART);
        assert!(capabilities.contains(Capabilities::RESTART));
        assert!(!capabilities.contains(Capabilities::SLEEP));
        assert!(Capabilities::NONE.contains(Capabilities::NONE));
    }
}
