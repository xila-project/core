//! UART device node control contract (`/devices/uart/<name>`).
//!
//! # Control commands (kind `b'u'`)
//!
//! | Number | Command      | Direction | Input        | Output       | Effect                              |
//! |--------|--------------|-----------|--------------|--------------|-------------------------------------|
//! | `0x00` | `GET_CONFIG` | Read      | `VersionAbi` | `ConfigAbi`  | Returns the current configuration.  |
//! | `0x01` | `SET_CONFIG` | Write     | `ConfigAbi`  | `VersionAbi` | Applies a configuration atomically. |
//! | `0x02` | `FLUSH_RX`   | Write     | `VersionAbi` | `VersionAbi` | Discards received, unread data.     |
//! | `0x03` | `FLUSH_TX`   | Write     | `VersionAbi` | `VersionAbi` | Discards queued, untransmitted data.|
//!
//! Every struct starts with a `version` field that must equal [`ABI_VERSION`];
//! any other value is `InvalidParameter`. Enum fields use the numeric codes
//! below; an unknown code is `InvalidParameter`, and a setting the driver
//! cannot express is `UnsupportedOperation`.

use file_system::{ControlCommand, define_command};

/// Version of every UART control ABI struct in this module.
pub const ABI_VERSION: u32 = 1;

/// Parity codes in [`ConfigAbi::parity`].
pub const PARITY_NONE: u8 = 0;
pub const PARITY_EVEN: u8 = 1;
pub const PARITY_ODD: u8 = 2;

/// Flow control codes in [`ConfigAbi::flow_control`].
pub const FLOW_CONTROL_NONE: u8 = 0;
pub const FLOW_CONTROL_RTS_CTS: u8 = 1;
pub const FLOW_CONTROL_XON_XOFF: u8 = 2;

/// Carries only the ABI version. Used as input and output of commands that
/// have no other payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionAbi {
    pub version: u32,
}

/// UART line configuration.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigAbi {
    pub version: u32,
    pub baud_rate: u32,
    /// 5, 6, 7 or 8.
    pub data_bits: u8,
    /// 1 or 2.
    pub stop_bits: u8,
    /// One of `PARITY_NONE`, `PARITY_EVEN`, `PARITY_ODD`.
    pub parity: u8,
    /// One of `FLOW_CONTROL_NONE`, `FLOW_CONTROL_RTS_CTS`, `FLOW_CONTROL_XON_XOFF`.
    pub flow_control: u8,
}

define_command!(
    GET_CONFIG,
    Read,
    crate::kind::UART,
    0x00,
    VersionAbi,
    ConfigAbi
);
define_command!(
    SET_CONFIG,
    Write,
    crate::kind::UART,
    0x01,
    ConfigAbi,
    VersionAbi
);
define_command!(
    FLUSH_RX,
    Write,
    crate::kind::UART,
    0x02,
    VersionAbi,
    VersionAbi
);
define_command!(
    FLUSH_TX,
    Write,
    crate::kind::UART,
    0x03,
    VersionAbi,
    VersionAbi
);
