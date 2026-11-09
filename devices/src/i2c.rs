//! I2C device node control contract (`/devices/i2c/<name>`).
//!
//! Control commands (kind `b'i'`), all with a `version` field first that must
//! equal [`ABI_VERSION`] and reserved fields that must be zero:
//! - `SELECT_ADDRESS` (write): 7-bit address, or 10-bit when the controller
//!   reports `ten_bit` support.
//! - `GET_FREQUENCY` (read): configured and maximum frequency.
//! - `SET_FREQUENCY` (write): new frequency, `0 < hz <= max_frequency_hz`.
//! - `WRITE_READ` (write): write then read in one transaction with repeated
//!   START, against the selected address. Lengths are at most
//!   [`MAX_TRANSFER_LENGTH`] and at least one of them must be non-zero.

use file_system::{ControlCommand, define_command};

/// Version carried in the first field of every command input and output.
pub const ABI_VERSION: u16 = 1;

/// Largest write or read length of one `WRITE_READ` transfer.
pub const MAX_TRANSFER_LENGTH: usize = 64;

/// Input of `SELECT_ADDRESS`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectAddressInput {
    pub version: u16,
    /// `0` selects a 7-bit address, `1` a 10-bit address.
    pub ten_bit: u8,
    pub reserved: u8,
    pub address: u16,
}

/// Output of `GET_FREQUENCY`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrequencyOutput {
    pub version: u16,
    /// `1` when the controller supports 10-bit addresses.
    pub ten_bit_supported: u8,
    pub reserved: u8,
    pub frequency_hz: u32,
    pub max_frequency_hz: u32,
}

/// Input of `SET_FREQUENCY`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetFrequencyInput {
    pub version: u16,
    pub reserved: u16,
    pub frequency_hz: u32,
}

/// Input of `WRITE_READ`. Only the first `write_length` bytes of `write` are sent.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteReadInput {
    pub version: u16,
    pub write_length: u8,
    pub read_length: u8,
    pub write: [u8; MAX_TRANSFER_LENGTH],
}

impl Default for WriteReadInput {
    fn default() -> Self {
        Self {
            version: ABI_VERSION,
            write_length: 0,
            read_length: 0,
            write: [0; MAX_TRANSFER_LENGTH],
        }
    }
}

/// Output of `WRITE_READ`. The first `read_length` bytes of `read` are valid.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteReadOutput {
    pub version: u16,
    pub read_length: u8,
    pub reserved: u8,
    pub read: [u8; MAX_TRANSFER_LENGTH],
}

impl Default for WriteReadOutput {
    fn default() -> Self {
        Self {
            version: ABI_VERSION,
            read_length: 0,
            reserved: 0,
            read: [0; MAX_TRANSFER_LENGTH],
        }
    }
}

define_command!(
    SELECT_ADDRESS,
    Write,
    crate::kind::I2C,
    0,
    SelectAddressInput,
    ()
);
define_command!(GET_FREQUENCY, Read, crate::kind::I2C, 1, (), FrequencyOutput);
define_command!(
    SET_FREQUENCY,
    Write,
    crate::kind::I2C,
    2,
    SetFrequencyInput,
    ()
);
define_command!(
    WRITE_READ,
    Write,
    crate::kind::I2C,
    3,
    WriteReadInput,
    WriteReadOutput
);
