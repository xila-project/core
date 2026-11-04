//! Xila SPI contract on top of `embedded-hal`.
//!
//! Drivers use `embedded_hal::spi::SpiDevice` (and `SpiBus` on a selected bus). Xila owns only
//! what embedded-hal lacks: reserving a shared bus for one exclusive transaction, naming
//! chip-select endpoints, per-device settings, per-segment overrides and the error mapping.

use embedded_hal::spi::Operation;

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::{BusCapabilities, SimulatedSpiDevice, SpiMock, SpiMockReservation, SpiMockSelected};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Mode0,
    Mode1,
    Mode2,
    Mode3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitOrder {
    MsbFirst,
    LsbFirst,
}

/// Per-device settings applied to the bus each time the device runs a transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceConfig {
    pub mode: Mode,
    pub frequency_hz: u32,
    pub bits_per_word: u8,
    pub bit_order: BitOrder,
}

impl DeviceConfig {
    pub const fn new(mode: Mode, frequency_hz: u32) -> Self {
        Self {
            mode,
            frequency_hz,
            bits_per_word: 8,
            bit_order: BitOrder::MsbFirst,
        }
    }
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self::new(Mode::Mode0, 1_000_000)
    }
}

/// Index of a chip-select endpoint on a bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChipSelect(u8);

impl ChipSelect {
    pub const fn new(index: u8) -> Self {
        Self(index)
    }

    pub const fn index(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpiError {
    /// The bus is already reserved for another transaction.
    Busy,
    /// The chip-select endpoint does not exist or has no device attached.
    InvalidChipSelect,
    ModeNotSupported,
    FrequencyNotSupported,
    BitsPerWordNotSupported,
    BitOrderNotSupported,
    /// A segment is malformed, for example a full-duplex operation with mismatched lengths.
    InvalidSegment,
    /// The operation cannot be executed on this bus (for example `DelayNs` without a delay).
    Unsupported,
}

impl embedded_hal::spi::Error for SpiError {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        use embedded_hal::spi::ErrorKind;
        match self {
            // Another master holds the bus: closest embedded-hal meaning is contention.
            Self::Busy => ErrorKind::ModeFault,
            Self::InvalidChipSelect => ErrorKind::ChipSelectFault,
            Self::BitsPerWordNotSupported | Self::BitOrderNotSupported => ErrorKind::FrameFormat,
            Self::ModeNotSupported
            | Self::FrequencyNotSupported
            | Self::InvalidSegment
            | Self::Unsupported => ErrorKind::Other,
        }
    }
}

/// One `embedded-hal` operation plus the Xila per-segment options.
#[derive(Debug, PartialEq, Eq)]
pub struct Segment<'a> {
    pub operation: Operation<'a, u8>,
    /// Overrides the device frequency for this segment only.
    pub frequency_hz: Option<u32>,
    /// Overrides the device word size for this segment only.
    pub bits_per_word: Option<u8>,
    /// Deasserts chip select after this segment. Otherwise chip select stays asserted.
    pub cs_change: bool,
}

impl<'a> Segment<'a> {
    pub const fn new(operation: Operation<'a, u8>) -> Self {
        Self {
            operation,
            frequency_hz: None,
            bits_per_word: None,
            cs_change: false,
        }
    }

    pub const fn with_frequency(mut self, frequency_hz: u32) -> Self {
        self.frequency_hz = Some(frequency_hz);
        self
    }

    pub const fn with_bits_per_word(mut self, bits_per_word: u8) -> Self {
        self.bits_per_word = Some(bits_per_word);
        self
    }

    pub const fn with_cs_change(mut self) -> Self {
        self.cs_change = true;
        self
    }
}

/// A shared bus that can be reserved for one exclusive transaction at a time.
pub trait SpiController {
    type Reservation<'a>: SpiReservation
    where
        Self: 'a;

    /// Reserves the bus. Fails with [`SpiError::Busy`] while another reservation is alive.
    fn reserve(&self) -> Result<Self::Reservation<'_>, SpiError>;
}

/// Exclusive access to a bus for the duration of one transaction.
pub trait SpiReservation {
    /// Asserts `chip_select`, applies `config`, runs `segments` in order, and deasserts chip select.
    fn transaction<'s, I>(
        &mut self,
        chip_select: ChipSelect,
        config: DeviceConfig,
        segments: I,
    ) -> Result<(), SpiError>
    where
        I: IntoIterator<Item = Segment<'s>>;
}

/// One logical device: a shared bus, its chip-select endpoint and its settings.
pub struct SpiDeviceHandle<'a, C: SpiController> {
    controller: &'a C,
    chip_select: ChipSelect,
    config: DeviceConfig,
}

impl<'a, C: SpiController> SpiDeviceHandle<'a, C> {
    pub const fn new(controller: &'a C, chip_select: ChipSelect, config: DeviceConfig) -> Self {
        Self {
            controller,
            chip_select,
            config,
        }
    }

    pub const fn chip_select(&self) -> ChipSelect {
        self.chip_select
    }

    pub const fn config(&self) -> DeviceConfig {
        self.config
    }

    /// Reserves the bus, runs one transaction for this device, and releases the bus.
    pub fn transaction<'s, I>(&self, segments: I) -> Result<(), SpiError>
    where
        I: IntoIterator<Item = Segment<'s>>,
    {
        let mut bus = self.controller.reserve()?;
        bus.transaction(self.chip_select, self.config, segments)
    }
}

impl<C: SpiController> embedded_hal::spi::ErrorType for SpiDeviceHandle<'_, C> {
    type Error = SpiError;
}

impl<C: SpiController> embedded_hal::spi::SpiDevice<u8> for SpiDeviceHandle<'_, C> {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), SpiError> {
        let segments = operations
            .iter_mut()
            .map(|operation| Segment::new(reborrow(operation)));
        SpiDeviceHandle::transaction(self, segments)
    }
}

/// Reborrows an operation for the lifetime of the mutable slot that holds it.
fn reborrow<'a>(operation: &'a mut Operation<'_, u8>) -> Operation<'a, u8> {
    match operation {
        Operation::Read(buffer) => Operation::Read(buffer),
        Operation::Write(buffer) => Operation::Write(buffer),
        Operation::Transfer(read, write) => Operation::Transfer(read, write),
        Operation::TransferInPlace(buffer) => Operation::TransferInPlace(buffer),
        Operation::DelayNs(nanoseconds) => Operation::DelayNs(*nanoseconds),
    }
}
