//! SPI device nodes: one `/devices/spi/spi<bus>.<cs>` per chip-select endpoint.
//!
//! Applications use plain `read`, `write`, `flush` and `close`. Plain `write`
//! sends a write-only transfer and plain `read` clocks zeros on MOSI, as the
//! `peripherals::spi` contract defines. Each call is one non-blocking attempt
//! on the shared bus.
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

use embedded_hal::spi::Operation;
use file_system::{
    ControlCommand, ControlCommandIdentifier, ControlDirectionFlags, Error, Result, define_command,
};
use peripherals::spi::{
    BitOrder, ChipSelect, DeviceConfig, Mode, Segment, SpiController, SpiDeviceHandle, SpiError,
};
use shared::AnyByLayout;

use crate::PeripheralHandle;

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

impl SpiConfigurationAbi {
    fn from_config(config: DeviceConfig) -> Self {
        Self {
            version: SPI_ABI_VERSION,
            mode: match config.mode {
                Mode::Mode0 => 0,
                Mode::Mode1 => 1,
                Mode::Mode2 => 2,
                Mode::Mode3 => 3,
            },
            bit_order: match config.bit_order {
                BitOrder::MsbFirst => 0,
                BitOrder::LsbFirst => 1,
            },
            frequency_hz: config.frequency_hz,
            bits_per_word: config.bits_per_word,
            reserved: [0; 3],
        }
    }

    fn to_config(self) -> Result<DeviceConfig> {
        if self.version != SPI_ABI_VERSION
            || self.reserved != [0; 3]
            || !(1..=32).contains(&self.bits_per_word)
        {
            return Err(Error::InvalidParameter);
        }
        let mode = match self.mode {
            0 => Mode::Mode0,
            1 => Mode::Mode1,
            2 => Mode::Mode2,
            3 => Mode::Mode3,
            _ => return Err(Error::InvalidParameter),
        };
        let bit_order = match self.bit_order {
            0 => BitOrder::MsbFirst,
            1 => BitOrder::LsbFirst,
            _ => return Err(Error::InvalidParameter),
        };
        Ok(DeviceConfig {
            mode,
            frequency_hz: self.frequency_hz,
            bits_per_word: self.bits_per_word,
            bit_order,
        })
    }
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
    b's',
    0,
    (),
    SpiConfigurationAbi
);
define_command!(
    SPI_SET_CONFIGURATION,
    Write,
    b's',
    1,
    SpiConfigurationAbi,
    ()
);
// Full-duplex transfer of up to SPI_MAX_TRANSFER_BYTES in one chip-select cycle.
define_command!(
    SPI_TRANSFER,
    ControlDirectionFlags::all(),
    b's',
    2,
    SpiTransferInputAbi,
    SpiTransferOutputAbi
);
// Atomic multi-segment transaction, honouring cs_change between segments.
define_command!(
    SPI_TRANSACTION,
    ControlDirectionFlags::all(),
    b's',
    3,
    SpiTransactionInputAbi,
    SpiTransactionOutputAbi
);

/// Maps a contract error to the closest file system error.
fn map_error(error: SpiError) -> Error {
    match error {
        SpiError::Busy => Error::RessourceBusy,
        SpiError::InvalidChipSelect => Error::NotFound,
        SpiError::ModeNotSupported
        | SpiError::FrequencyNotSupported
        | SpiError::BitsPerWordNotSupported
        | SpiError::BitOrderNotSupported
        | SpiError::Unsupported => Error::UnsupportedOperation,
        SpiError::InvalidSegment => Error::InvalidParameter,
    }
}

/// Checks the header of a transaction input and returns (segment count, total length).
fn validate_transaction(input: &SpiTransactionInputAbi) -> Result<(usize, usize)> {
    if input.version != SPI_ABI_VERSION || input.reserved != 0 {
        return Err(Error::InvalidParameter);
    }
    let count = usize::from(input.segment_count);
    if count == 0 || count > SPI_MAX_SEGMENTS {
        return Err(Error::InvalidParameter);
    }

    let mut total = 0usize;
    for segment in &input.segments[..count] {
        if !matches!(
            segment.kind,
            SPI_SEGMENT_WRITE | SPI_SEGMENT_READ | SPI_SEGMENT_TRANSFER
        ) || segment.cs_change > 1
        {
            return Err(Error::InvalidParameter);
        }
        total += usize::from(segment.length);
    }
    if total > SPI_MAX_TRANSACTION_BYTES {
        return Err(Error::InvalidParameter);
    }
    Ok((count, total))
}

/// Builds segments as slices of the input and output data arrays, without allocation.
///
/// Both slices must be at least as long as the sum of the segment lengths, which
/// [`validate_transaction`] guarantees before this iterator is created.
struct SegmentIterator<'i, 'o> {
    segments: core::slice::Iter<'i, SpiSegmentAbi>,
    input: &'i [u8],
    output: &'o mut [u8],
}

impl<'i: 'o, 'o> Iterator for SegmentIterator<'i, 'o> {
    type Item = Segment<'o>;

    fn next(&mut self) -> Option<Self::Item> {
        let segment = *self.segments.next()?;
        let length = usize::from(segment.length);

        let (input_head, input_tail) = self.input.split_at(length);
        let (output_head, output_tail) = core::mem::take(&mut self.output).split_at_mut(length);
        self.input = input_tail;
        self.output = output_tail;

        let operation = match segment.kind {
            SPI_SEGMENT_WRITE => Operation::Write(input_head),
            SPI_SEGMENT_READ => Operation::Read(output_head),
            _ => Operation::Transfer(output_head, input_head),
        };
        let mut segment_out = Segment::new(operation);
        if segment.cs_change != 0 {
            segment_out = segment_out.with_cs_change();
        }
        Some(segment_out)
    }
}

/// SPI device node: one chip-select endpoint of a shared bus with its settings.
///
/// The bus is borrowed for `'static` so several nodes can share one controller.
/// Each read, write or control call reserves the bus once and never waits.
pub struct SpiDevice<C: SpiController + 'static> {
    controller: &'static C,
    chip_select: ChipSelect,
    config: DeviceConfig,
}

impl<C: SpiController + 'static> SpiDevice<C> {
    pub const fn new(
        controller: &'static C,
        chip_select: ChipSelect,
        config: DeviceConfig,
    ) -> Self {
        Self {
            controller,
            chip_select,
            config,
        }
    }

    fn run<'s, I>(&self, segments: I) -> Result<()>
    where
        I: IntoIterator<Item = Segment<'s>>,
    {
        SpiDeviceHandle::new(self.controller, self.chip_select, self.config)
            .transaction(segments)
            .map_err(map_error)
    }

    /// Validates `config` through the contract's capability checks. The empty
    /// transaction applies no segment, so no data moves on the bus.
    fn probe(&self, config: DeviceConfig) -> Result<()> {
        SpiDeviceHandle::new(self.controller, self.chip_select, config)
            .transaction(core::iter::empty::<Segment<'_>>())
            .map_err(map_error)
    }
}

impl<C: SpiController + Sync + 'static> PeripheralHandle for SpiDevice<C> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let length = buffer.len();
        self.run([Segment::new(Operation::Read(buffer))])?;
        Ok(length)
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.run([Segment::new(Operation::Write(buffer))])?;
        Ok(buffer.len())
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == SPI_GET_CONFIGURATION::IDENTIFIER {
            // The input is `()`, so only the output is cast.
            let output = SPI_GET_CONFIGURATION::cast_output(output)?;
            *output = SpiConfigurationAbi::from_config(self.config);
            Ok(())
        } else if command == SPI_SET_CONFIGURATION::IDENTIFIER {
            let config = SPI_SET_CONFIGURATION::cast_input(input)?.to_config()?;
            self.probe(config)?;
            self.config = config;
            Ok(())
        } else if command == SPI_TRANSFER::IDENTIFIER {
            let (input, output) = SPI_TRANSFER::cast(input, output)?;
            let length = usize::from(input.length);
            if input.version != SPI_ABI_VERSION || length > SPI_MAX_TRANSFER_BYTES {
                return Err(Error::InvalidParameter);
            }

            output.version = SPI_ABI_VERSION;
            output.length = input.length;
            output.rx = [0; SPI_MAX_TRANSFER_BYTES];
            self.run([Segment::new(Operation::Transfer(
                &mut output.rx[..length],
                &input.tx[..length],
            ))])
        } else if command == SPI_TRANSACTION::IDENTIFIER {
            let (input, output) = SPI_TRANSACTION::cast(input, output)?;
            let (count, total) = validate_transaction(input)?;

            output.version = SPI_ABI_VERSION;
            output.length = total as u16;
            output.data = [0; SPI_MAX_TRANSACTION_BYTES];
            let segments = SegmentIterator {
                segments: input.segments[..count].iter(),
                input: &input.data[..total],
                output: &mut output.data[..total],
            };
            // One reservation covers every segment, so no other master interleaves.
            self.run(segments)
        } else {
            Err(Error::UnsupportedOperation)
        }
    }
}

/// Mounts the SPI device `spi<bus>.<chip_select>` at `/devices/spi/spi<bus>.<chip_select>`.
///
/// Creates `/devices` and `/devices/spi` when missing. `config` is the initial
/// device configuration. A chip select without an endpoint on the bus fails
/// on first use with `NotFound`, not at mount time.
#[cfg(feature = "vfs")]
pub async fn mount_spi_device<C>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    bus: u8,
    chip_select: u8,
    controller: &'static C,
    config: DeviceConfig,
) -> virtual_file_system::Result<()>
where
    C: SpiController + Sync + 'static,
{
    use file_system::Path;

    for directory in [Path::DEVICES, Path::SPI_DEVICES] {
        match virtual_file_system.create_directory(task, &directory).await {
            Ok(()) | Err(virtual_file_system::Error::AlreadyExists) => {}
            Err(error) => return Err(error),
        }
    }

    let path = alloc::format!("{}/spi{bus}.{chip_select}", Path::SPI_DEVICES.as_str());
    crate::mount_peripheral(
        virtual_file_system,
        task,
        path.as_str(),
        SpiDevice::new(controller, ChipSelect::new(chip_select), config),
    )
    .await
}
