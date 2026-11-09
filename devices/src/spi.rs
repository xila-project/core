//! SPI device node control contract (`/devices/spi/spi<bus>.<cs>`).
//!
//! Configuration and multi-segment transfers use the control commands below,
//! all with kind byte `b's'`. Every input and output struct is `#[repr(C)]`,
//! starts with a `version` field that must equal [`SPI_ABI_VERSION`], and is
//! validated before use.
//!
//! Bounds (the transaction path never allocates):
//! - [`SPI_TRANSFER`] moves at most [`SPI_MAX_TRANSFER_BYTES`] bytes full duplex.
//! - [`SPI_TRANSACTION`] runs at most [`SPI_MAX_SEGMENTS`] segments whose
//!   lengths sum to at most [`SPI_MAX_TRANSACTION_BYTES`]. Segment data is laid
//!   out back to back in both the input and output `data` arrays, so segment
//!   `n` starts at the sum of the lengths of segments `0..n`.

use file_system::{ControlCommand, ControlDirectionFlags, define_command};

/// ABI version carried by every command input and output struct.
pub const SPI_ABI_VERSION: u16 = 1;
/// Maximum bytes moved by one [`SPI_TRANSFER`].
pub const SPI_MAX_TRANSFER_BYTES: usize = 64;
/// Maximum segments in one [`SPI_TRANSACTION`].
pub const SPI_MAX_SEGMENTS: usize = 8;
/// Maximum sum of segment lengths in one [`SPI_TRANSACTION`].
pub const SPI_MAX_TRANSACTION_BYTES: usize = 64;

/// [`SpiSegmentAbi::kind`] value for a write-only segment.
pub const SPI_SEGMENT_WRITE: u8 = 0;
/// [`SpiSegmentAbi::kind`] value for a read segment (zeros on MOSI).
pub const SPI_SEGMENT_READ: u8 = 1;
/// [`SpiSegmentAbi::kind`] value for a full-duplex segment.
pub const SPI_SEGMENT_TRANSFER: u8 = 2;

/// Device settings. Mode is 0..=3, bit order is 0 (MSB first) or 1 (LSB first).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpiConfigurationAbi {
    pub version: u16,
    pub mode: u8,
    pub bit_order: u8,
    pub frequency_hz: u32,
    /// Must be 1..=32.
    pub bits_per_word: u8,
    /// Must be zero.
    pub reserved: [u8; 3],
}

/// Input of [`SPI_TRANSFER`]: `length` bytes of `tx` are sent while the same
/// number of bytes is received into [`SpiTransferOutputAbi::rx`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiTransferInputAbi {
    pub version: u16,
    pub length: u16,
    pub tx: [u8; SPI_MAX_TRANSFER_BYTES],
}

/// Output of [`SPI_TRANSFER`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiTransferOutputAbi {
    pub version: u16,
    pub length: u16,
    pub rx: [u8; SPI_MAX_TRANSFER_BYTES],
}

impl Default for SpiTransferOutputAbi {
    fn default() -> Self {
        Self {
            version: 0,
            length: 0,
            rx: [0; SPI_MAX_TRANSFER_BYTES],
        }
    }
}

/// One segment of [`SPI_TRANSACTION`]. `kind` is one of the `SPI_SEGMENT_*`
/// values and `cs_change` is 0 or 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpiSegmentAbi {
    pub kind: u8,
    pub cs_change: u8,
    pub length: u16,
}

/// Input of [`SPI_TRANSACTION`]. Only the first `segment_count` segments are
/// read. `reserved` must be zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiTransactionInputAbi {
    pub version: u16,
    pub segment_count: u8,
    pub reserved: u8,
    pub segments: [SpiSegmentAbi; SPI_MAX_SEGMENTS],
    pub data: [u8; SPI_MAX_TRANSACTION_BYTES],
}

/// Output of [`SPI_TRANSACTION`]. `length` is the total data length. Bytes of
/// write-only segments are zero.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiTransactionOutputAbi {
    pub version: u16,
    pub length: u16,
    pub data: [u8; SPI_MAX_TRANSACTION_BYTES],
}

impl Default for SpiTransactionOutputAbi {
    fn default() -> Self {
        Self {
            version: 0,
            length: 0,
            data: [0; SPI_MAX_TRANSACTION_BYTES],
        }
    }
}

// Get and set the device configuration. Getting takes no input.
define_command!(
    SPI_GET_CONFIGURATION,
    Read,
    crate::kind::SPI,
    0,
    (),
    SpiConfigurationAbi
);
define_command!(
    SPI_SET_CONFIGURATION,
    Write,
    crate::kind::SPI,
    1,
    SpiConfigurationAbi,
    ()
);
// Full-duplex transfer of up to SPI_MAX_TRANSFER_BYTES in one chip-select cycle.
define_command!(
    SPI_TRANSFER,
    ControlDirectionFlags::all(),
    crate::kind::SPI,
    2,
    SpiTransferInputAbi,
    SpiTransferOutputAbi
);
// Atomic multi-segment transaction, honouring cs_change between segments.
define_command!(
    SPI_TRANSACTION,
    ControlDirectionFlags::all(),
    crate::kind::SPI,
    3,
    SpiTransactionInputAbi,
    SpiTransactionOutputAbi
);
