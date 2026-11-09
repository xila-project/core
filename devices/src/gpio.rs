//! GPIO line control contract (`/devices/gpio/<name>`).
//!
//! # Control commands (kind `b'g'`)
//!
//! | Number | Command           | Direction | Input           | Output       | Effect                                              |
//! |--------|-------------------|-----------|-----------------|--------------|-----------------------------------------------------|
//! | `0x00` | `CONFIGURE`       | Write     | `ConfigureAbi`  | `VersionAbi` | Releases the line, then requests it as input/output. |
//! | `0x01` | `RELEASE`         | Write     | `VersionAbi`    | `VersionAbi` | Releases the line, returning it to high impedance.  |
//! | `0x02` | `GET_CAPABILITIES`| Read      | `VersionAbi`    | `CapabilitiesAbi` | Reports what the line supports.                  |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and its
//! reserved fields must be zero; otherwise the command fails with
//! `InvalidParameter`. Field values outside their documented range are
//! `InvalidParameter`. A controller refusal to take the line (unsupported
//! option, line already in use) is reported as `InputOutput`.

use file_system::{ControlCommand, define_command};

/// Version of every GPIO control ABI struct in this module.
pub const ABI_VERSION: u16 = 1;

/// Carries only the ABI version. Used as input of commands without payload and
/// as output of commands that report success.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionAbi {
    pub version: u16,
    pub reserved: u16,
}

/// Requested configuration of the line.
///
/// - `direction`: `0` input, `1` output.
/// - `pull`: `0` none, `1` pull up, `2` pull down.
/// - `drive_mode`: `0` push-pull, `1` open-drain. Outputs only.
/// - `initial_level`: `0` low, `1` high. Outputs only.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigureAbi {
    pub version: u16,
    pub direction: u8,
    pub pull: u8,
    pub drive_mode: u8,
    pub initial_level: u8,
    pub reserved: u16,
}

/// Line capabilities; each flag is `0` or `1`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapabilitiesAbi {
    pub version: u16,
    pub input: u8,
    pub output: u8,
    pub pull_up: u8,
    pub pull_down: u8,
    pub open_drain: u8,
    pub reserved: u8,
}

define_command!(
    CONFIGURE,
    Write,
    crate::kind::GPIO,
    0x00,
    ConfigureAbi,
    VersionAbi
);
define_command!(
    RELEASE,
    Write,
    crate::kind::GPIO,
    0x01,
    VersionAbi,
    VersionAbi
);
define_command!(
    GET_CAPABILITIES,
    Read,
    crate::kind::GPIO,
    0x02,
    VersionAbi,
    CapabilitiesAbi
);
