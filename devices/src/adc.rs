//! ADC device node control contract (`/devices/adc/<name>`).
//!
//! # Control commands (kind `b'a'`)
//!
//! | Number | Command           | Direction | Input              | Output             | Effect                                        |
//! |--------|-------------------|-----------|--------------------|--------------------|-----------------------------------------------|
//! | `0x00` | `GET_INFO`        | Read      | `VersionAbi`       | `InfoAbi`          | Returns channels, resolution and reference.   |
//! | `0x01` | `SELECT_CHANNEL`  | Write     | `SelectChannelAbi` | `VersionAbi`       | Selects the channel used by plain reads.      |
//! | `0x02` | `READ_MILLIVOLTS` | Read      | `VersionAbi`       | `MillivoltsAbi`    | Converts the selected channel to millivolts.  |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and its
//! reserved fields must be zero; otherwise the command fails with
//! `InvalidParameter`. A channel outside `0..channels` is `InvalidParameter`,
//! and `READ_MILLIVOLTS` on a converter without a reference voltage is
//! `UnsupportedOperation`.

use file_system::{ControlCommand, define_command};

/// Version of every ADC control ABI struct in this module.
pub const ABI_VERSION: u16 = 1;

/// Carries only the ABI version. Used as input of queries and output of commands
/// that have no other payload.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionAbi {
    pub version: u16,
    pub reserved: u16,
}

/// Converter geometry. `reference_millivolts` is meaningful only when
/// `reference_present` is 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InfoAbi {
    pub version: u16,
    pub channels: u8,
    pub resolution_bits: u8,
    pub reference_millivolts: u32,
    pub reference_present: u8,
    pub reserved: [u8; 3],
}

/// Selects the channel used by plain reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectChannelAbi {
    pub version: u16,
    pub channel: u8,
    pub reserved: u8,
}

/// One conversion in millivolts.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MillivoltsAbi {
    pub version: u16,
    pub reserved: u16,
    pub millivolts: u32,
}

define_command!(GET_INFO, Read, crate::kind::ADC, 0x00, VersionAbi, InfoAbi);
define_command!(
    SELECT_CHANNEL,
    Write,
    crate::kind::ADC,
    0x01,
    SelectChannelAbi,
    VersionAbi
);
define_command!(
    READ_MILLIVOLTS,
    Read,
    crate::kind::ADC,
    0x02,
    VersionAbi,
    MillivoltsAbi
);
