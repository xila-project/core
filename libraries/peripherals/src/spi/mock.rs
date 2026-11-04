use core::sync::atomic::{AtomicBool, Ordering};

use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use embedded_hal::spi::Operation;

use super::SpiReservation;
use super::{BitOrder, ChipSelect, DeviceConfig, Mode, Segment, SpiController, SpiError};

/// Bytes moved per simulated call, kept on the stack.
const CHUNK: usize = 32;

/// A device attached to a virtual chip-select endpoint. Devices are `Send` so the endpoint
/// can sit behind a mutex.
pub trait SimulatedSpiDevice: Send {
    /// Called with `true` when chip select is asserted and `false` when it is released.
    fn chip_select(&mut self, _asserted: bool) {}

    /// Full duplex: receives `tx` and fills `rx` with the response (same length, at most 32).
    fn transfer(&mut self, tx: &[u8], rx: &mut [u8]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusCapabilities {
    pub max_frequency_hz: u32,
    /// Supported modes, indexed Mode0..Mode3.
    pub modes: [bool; 4],
    pub lsb_first: bool,
}

impl BusCapabilities {
    /// Mode 0 only, MSB first; the mock supports 8-bit words only.
    pub const fn new(max_frequency_hz: u32) -> Self {
        Self {
            max_frequency_hz,
            modes: [true, false, false, false],
            lsb_first: false,
        }
    }

    fn validate_config(&self, config: DeviceConfig) -> Result<(), SpiError> {
        let mode_index = match config.mode {
            Mode::Mode0 => 0,
            Mode::Mode1 => 1,
            Mode::Mode2 => 2,
            Mode::Mode3 => 3,
        };
        if !self.modes[mode_index] {
            return Err(SpiError::ModeNotSupported);
        }
        self.validate_frequency(config.frequency_hz)?;
        self.validate_word_size(config.bits_per_word)?;
        if config.bit_order == BitOrder::LsbFirst && !self.lsb_first {
            return Err(SpiError::BitOrderNotSupported);
        }
        Ok(())
    }

    fn validate_frequency(&self, frequency_hz: u32) -> Result<(), SpiError> {
        if frequency_hz == 0 || frequency_hz > self.max_frequency_hz {
            Err(SpiError::FrequencyNotSupported)
        } else {
            Ok(())
        }
    }

    fn validate_word_size(&self, bits_per_word: u8) -> Result<(), SpiError> {
        if bits_per_word == 8 {
            Ok(())
        } else {
            Err(SpiError::BitsPerWordNotSupported)
        }
    }
}

type Endpoint<'d> = Option<&'d Mutex<CriticalSectionRawMutex, dyn SimulatedSpiDevice + 'd>>;

/// Virtual SPI bus with `N` chip-select endpoints. Each endpoint is either empty or holds a
/// simulated device. At most one reservation can be alive, so transactions never interleave.
pub struct SpiMock<'d, const N: usize> {
    capabilities: BusCapabilities,
    endpoints: [Endpoint<'d>; N],
    reserved: AtomicBool,
}

impl<'d, const N: usize> SpiMock<'d, N> {
    pub fn new(capabilities: BusCapabilities, endpoints: [Endpoint<'d>; N]) -> Self {
        Self {
            capabilities,
            endpoints,
            reserved: AtomicBool::new(false),
        }
    }

    fn endpoint(
        &self,
        chip_select: ChipSelect,
    ) -> Result<&'d Mutex<CriticalSectionRawMutex, dyn SimulatedSpiDevice + 'd>, SpiError> {
        self.endpoints
            .get(usize::from(chip_select.index()))
            .copied()
            .flatten()
            .ok_or(SpiError::InvalidChipSelect)
    }
}

pub struct SpiMockReservation<'a, 'd, const N: usize> {
    bus: &'a SpiMock<'d, N>,
}

impl<'a, 'd, const N: usize> SpiMockReservation<'a, 'd, N> {
    /// Asserts chip select on `chip_select` and returns a bus view that holds the reservation
    /// until dropped, when chip select is released. Implements `embedded_hal::spi::SpiBus`.
    pub fn select(
        self,
        chip_select: ChipSelect,
        config: DeviceConfig,
    ) -> Result<SpiMockSelected<'a, 'd, N>, SpiError> {
        let capabilities = self.bus.capabilities;
        capabilities.validate_config(config)?;
        let endpoint = self.bus.endpoint(chip_select)?;
        let mut device = endpoint
            .try_lock()
            .expect("SPI mock endpoint is already borrowed");
        device.chip_select(true);
        Ok(SpiMockSelected {
            device,
            capabilities,
            config,
            _reservation: self,
        })
    }
}

impl<const N: usize> Drop for SpiMockReservation<'_, '_, N> {
    fn drop(&mut self) {
        self.bus.reserved.store(false, Ordering::Release);
    }
}

impl<'d, const N: usize> SpiController for SpiMock<'d, N> {
    type Reservation<'a>
        = SpiMockReservation<'a, 'd, N>
    where
        Self: 'a;

    fn reserve(&self) -> Result<Self::Reservation<'_>, SpiError> {
        if self.reserved.swap(true, Ordering::AcqRel) {
            return Err(SpiError::Busy);
        }
        Ok(SpiMockReservation { bus: self })
    }
}

impl<const N: usize> SpiReservation for SpiMockReservation<'_, '_, N> {
    fn transaction<'s, I>(
        &mut self,
        chip_select: ChipSelect,
        config: DeviceConfig,
        segments: I,
    ) -> Result<(), SpiError>
    where
        I: IntoIterator<Item = Segment<'s>>,
    {
        let capabilities = self.bus.capabilities;
        capabilities.validate_config(config)?;
        let endpoint = self.bus.endpoint(chip_select)?;
        let mut device = endpoint
            .try_lock()
            .expect("SPI mock endpoint is already borrowed");
        let mut asserted = false;

        let result = run_segments(capabilities, config, &mut *device, &mut asserted, segments);
        if asserted {
            device.chip_select(false);
        }
        result
    }
}

/// A reserved bus with one chip-select endpoint asserted. Chip select is released on drop.
pub struct SpiMockSelected<'a, 'd, const N: usize> {
    device: MutexGuard<'d, CriticalSectionRawMutex, dyn SimulatedSpiDevice + 'd>,
    capabilities: BusCapabilities,
    config: DeviceConfig,
    _reservation: SpiMockReservation<'a, 'd, N>,
}

impl<const N: usize> Drop for SpiMockSelected<'_, '_, N> {
    fn drop(&mut self) {
        self.device.chip_select(false);
    }
}

impl<const N: usize> SpiMockSelected<'_, '_, N> {
    fn run(&mut self, operation: Operation<'_, u8>) -> Result<(), SpiError> {
        // Chip select is already asserted for the whole selection, so the segment does not
        // toggle it.
        let mut asserted = true;
        run_segments(
            self.capabilities,
            self.config,
            &mut *self.device,
            &mut asserted,
            [Segment::new(operation)],
        )
    }
}

impl<const N: usize> embedded_hal::spi::ErrorType for SpiMockSelected<'_, '_, N> {
    type Error = SpiError;
}

impl<const N: usize> embedded_hal::spi::SpiBus<u8> for SpiMockSelected<'_, '_, N> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), SpiError> {
        self.run(Operation::Read(words))
    }

    fn write(&mut self, words: &[u8]) -> Result<(), SpiError> {
        self.run(Operation::Write(words))
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), SpiError> {
        self.run(Operation::Transfer(read, write))
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), SpiError> {
        self.run(Operation::TransferInPlace(words))
    }

    fn flush(&mut self) -> Result<(), SpiError> {
        Ok(())
    }
}

fn run_segments<'s>(
    capabilities: BusCapabilities,
    config: DeviceConfig,
    device: &mut dyn SimulatedSpiDevice,
    asserted: &mut bool,
    segments: impl IntoIterator<Item = Segment<'s>>,
) -> Result<(), SpiError> {
    for segment in segments {
        capabilities.validate_frequency(segment.frequency_hz.unwrap_or(config.frequency_hz))?;
        capabilities.validate_word_size(segment.bits_per_word.unwrap_or(config.bits_per_word))?;
        match &segment.operation {
            Operation::Transfer(read, write) if read.len() != write.len() => {
                return Err(SpiError::InvalidSegment);
            }
            Operation::DelayNs(_) => return Err(SpiError::Unsupported),
            _ => {}
        }

        if !*asserted {
            device.chip_select(true);
            *asserted = true;
        }
        transfer_operation(device, segment.operation);

        if segment.cs_change {
            device.chip_select(false);
            *asserted = false;
        }
    }
    Ok(())
}

fn transfer_operation(device: &mut dyn SimulatedSpiDevice, operation: Operation<'_, u8>) {
    let zeros = [0u8; CHUNK];
    let mut scratch = [0u8; CHUNK];
    match operation {
        Operation::Read(rx) => {
            for chunk in rx.chunks_mut(CHUNK) {
                let length = chunk.len();
                device.transfer(&zeros[..length], chunk);
            }
        }
        Operation::Write(tx) => {
            for chunk in tx.chunks(CHUNK) {
                let length = chunk.len();
                device.transfer(chunk, &mut scratch[..length]);
            }
        }
        Operation::Transfer(rx, tx) => {
            for (rx_chunk, tx_chunk) in rx.chunks_mut(CHUNK).zip(tx.chunks(CHUNK)) {
                device.transfer(tx_chunk, rx_chunk);
            }
        }
        Operation::TransferInPlace(buffer) => {
            for chunk in buffer.chunks_mut(CHUNK) {
                let length = chunk.len();
                scratch[..length].copy_from_slice(chunk);
                device.transfer(&scratch[..length], chunk);
            }
        }
        // Rejected by `run_segments` before any chip-select activity.
        Operation::DelayNs(_) => {}
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use embedded_hal::spi::{ErrorKind, Operation, SpiBus};
    use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

    use super::{BusCapabilities, SimulatedSpiDevice, SpiMock};
    use crate::spi::{
        BitOrder, ChipSelect, DeviceConfig, Mode, Segment, SpiController, SpiDeviceHandle,
        SpiError, SpiReservation,
    };

    /// Records chip-select edges and traffic; answers each byte with `byte + 1`.
    struct Recorder {
        asserts: u8,
        deasserts: u8,
        received: [u8; 64],
        received_len: usize,
    }

    impl Recorder {
        const fn new() -> Self {
            Self {
                asserts: 0,
                deasserts: 0,
                received: [0; 64],
                received_len: 0,
            }
        }

        fn received(&self) -> &[u8] {
            &self.received[..self.received_len]
        }
    }

    impl SimulatedSpiDevice for Recorder {
        fn chip_select(&mut self, asserted: bool) {
            if asserted {
                self.asserts += 1;
            } else {
                self.deasserts += 1;
            }
        }

        fn transfer(&mut self, tx: &[u8], rx: &mut [u8]) {
            for (out, byte) in rx.iter_mut().zip(tx) {
                *out = byte.wrapping_add(1);
                self.received[self.received_len] = *byte;
                self.received_len += 1;
            }
        }
    }

    const CAPABILITIES: BusCapabilities = BusCapabilities {
        max_frequency_hz: 10_000_000,
        modes: [true, false, true, false],
        lsb_first: false,
    };

    fn config() -> DeviceConfig {
        DeviceConfig::new(Mode::Mode0, 1_000_000)
    }

    #[test]
    fn full_duplex_returns_device_response() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut rx = [0u8; 3];
        bus.reserve()
            .unwrap()
            .transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::Transfer(&mut rx, &[1, 2, 3]))],
            )
            .unwrap();
        assert_eq!(rx, [2, 3, 4]);
    }

    #[test]
    fn write_only_discards_response() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        bus.reserve()
            .unwrap()
            .transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::Write(&[7, 8]))],
            )
            .unwrap();
        assert_eq!(device.try_lock().unwrap().received(), &[7, 8]);
    }

    #[test]
    fn read_only_clocks_zeros_and_stores_response() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut rx = [0u8; 2];
        bus.reserve()
            .unwrap()
            .transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::Read(&mut rx))],
            )
            .unwrap();
        assert_eq!(rx, [1, 1]);
        assert_eq!(device.try_lock().unwrap().received(), &[0, 0]);
    }

    #[test]
    fn chip_select_is_held_across_segments() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        bus.reserve()
            .unwrap()
            .transaction(
                ChipSelect::new(0),
                config(),
                [
                    Segment::new(Operation::Write(&[1])),
                    Segment::new(Operation::Write(&[2])),
                ],
            )
            .unwrap();
        let device = device.try_lock().unwrap();
        assert_eq!((device.asserts, device.deasserts), (1, 1));
    }

    #[test]
    fn cs_change_deasserts_between_segments() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        bus.reserve()
            .unwrap()
            .transaction(
                ChipSelect::new(0),
                config(),
                [
                    Segment::new(Operation::Write(&[1])).with_cs_change(),
                    Segment::new(Operation::Write(&[2])),
                ],
            )
            .unwrap();
        let device = device.try_lock().unwrap();
        assert_eq!((device.asserts, device.deasserts), (2, 2));
    }

    #[test]
    fn large_in_place_transfer_is_chunked() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut buffer = [5u8; 40];
        bus.reserve()
            .unwrap()
            .transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::TransferInPlace(&mut buffer))],
            )
            .unwrap();
        assert_eq!(buffer, [6u8; 40]);
        assert_eq!(device.try_lock().unwrap().received(), &[5u8; 40]);
    }

    #[test]
    fn unsupported_mode_is_rejected_before_any_transfer() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut reservation = bus.reserve().unwrap();
        let result = reservation.transaction(
            ChipSelect::new(0),
            DeviceConfig::new(Mode::Mode1, 1_000_000),
            [Segment::new(Operation::Write(&[1]))],
        );
        assert_eq!(result, Err(SpiError::ModeNotSupported));
        assert_eq!(device.try_lock().unwrap().asserts, 0);
    }

    #[test]
    fn unsupported_frequency_is_rejected() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut reservation = bus.reserve().unwrap();
        assert_eq!(
            reservation.transaction(
                ChipSelect::new(0),
                DeviceConfig::new(Mode::Mode0, 20_000_000),
                [Segment::new(Operation::Write(&[1]))],
            ),
            Err(SpiError::FrequencyNotSupported)
        );
        assert_eq!(
            reservation.transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::Write(&[1])).with_frequency(0)],
            ),
            Err(SpiError::FrequencyNotSupported)
        );
    }

    #[test]
    fn unsupported_word_size_and_bit_order_are_rejected() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut reservation = bus.reserve().unwrap();
        assert_eq!(
            reservation.transaction(
                ChipSelect::new(0),
                DeviceConfig {
                    bits_per_word: 16,
                    ..config()
                },
                [Segment::new(Operation::Write(&[1]))],
            ),
            Err(SpiError::BitsPerWordNotSupported)
        );
        assert_eq!(
            reservation.transaction(
                ChipSelect::new(0),
                DeviceConfig {
                    bit_order: BitOrder::LsbFirst,
                    ..config()
                },
                [Segment::new(Operation::Write(&[1]))],
            ),
            Err(SpiError::BitOrderNotSupported)
        );
    }

    #[test]
    fn mismatched_full_duplex_lengths_are_rejected() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut rx = [0u8; 2];
        assert_eq!(
            bus.reserve().unwrap().transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::Transfer(&mut rx, &[1, 2, 3]))],
            ),
            Err(SpiError::InvalidSegment)
        );
    }

    #[test]
    fn delay_operation_is_unsupported_without_chip_select_activity() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        assert_eq!(
            bus.reserve().unwrap().transaction(
                ChipSelect::new(0),
                config(),
                [Segment::new(Operation::DelayNs(10))],
            ),
            Err(SpiError::Unsupported)
        );
        assert_eq!(device.try_lock().unwrap().asserts, 0);
    }

    #[test]
    fn second_reservation_is_rejected_until_first_is_dropped() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let reservation = bus.reserve().unwrap();
        assert!(matches!(bus.reserve(), Err(SpiError::Busy)));
        drop(reservation);
        assert!(bus.reserve().is_ok());
    }

    #[test]
    fn device_handle_is_rejected_while_bus_is_reserved() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let handle = SpiDeviceHandle::new(&bus, ChipSelect::new(0), config());
        let _held = bus.reserve().unwrap();
        assert_eq!(
            handle.transaction([Segment::new(Operation::Write(&[1]))]),
            Err(SpiError::Busy)
        );
    }

    #[test]
    fn invalid_or_empty_chip_select_is_rejected() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut reservation = bus.reserve().unwrap();
        assert_eq!(
            reservation.transaction(
                ChipSelect::new(2),
                config(),
                [Segment::new(Operation::Write(&[1]))]
            ),
            Err(SpiError::InvalidChipSelect)
        );
        assert_eq!(
            reservation.transaction(
                ChipSelect::new(1),
                config(),
                [Segment::new(Operation::Write(&[1]))]
            ),
            Err(SpiError::InvalidChipSelect)
        );
    }

    #[test]
    fn error_kinds_map_to_embedded_hal() {
        use embedded_hal::spi::Error as _;
        assert_eq!(SpiError::Busy.kind(), ErrorKind::ModeFault);
        assert_eq!(
            SpiError::InvalidChipSelect.kind(),
            ErrorKind::ChipSelectFault
        );
        assert_eq!(
            SpiError::BitOrderNotSupported.kind(),
            ErrorKind::FrameFormat
        );
        assert_eq!(SpiError::Unsupported.kind(), ErrorKind::Other);
        assert_eq!(SpiError::ModeNotSupported.kind(), ErrorKind::Other);
    }

    /// A third-party style driver written only against embedded-hal.
    fn probe<D: embedded_hal::spi::SpiDevice>(device: &mut D) -> Result<[u8; 2], D::Error> {
        device.write(&[0x9F])?;
        let mut id = [0u8; 2];
        device.transfer_in_place(&mut id)?;
        Ok(id)
    }

    #[test]
    fn embedded_hal_device_driver_runs_through_adapter() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut handle = SpiDeviceHandle::new(&bus, ChipSelect::new(0), config());
        let id = probe(&mut handle).unwrap();
        assert_eq!(id, [1, 1]);
        // One chip-select cycle per driver call: write, then in-place transfer.
        assert_eq!(device.try_lock().unwrap().asserts, 2);
    }

    /// A third-party style driver written only against the embedded-hal bus trait.
    fn probe_bus<B: SpiBus<u8>>(bus: &mut B) -> Result<[u8; 1], B::Error> {
        bus.write(&[0x42])?;
        let mut answer = [0u8; 1];
        bus.transfer_in_place(&mut answer)?;
        bus.flush()?;
        Ok(answer)
    }

    #[test]
    fn selected_bus_implements_embedded_hal_spi_bus() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut selected = bus
            .reserve()
            .unwrap()
            .select(ChipSelect::new(0), config())
            .unwrap();
        assert_eq!(probe_bus(&mut selected), Ok([1]));
        assert!(matches!(bus.reserve(), Err(SpiError::Busy)));
        drop(selected);

        let device = device.try_lock().unwrap();
        assert_eq!((device.asserts, device.deasserts), (1, 1));
        drop(device);
        assert!(bus.reserve().is_ok());
    }

    #[test]
    fn adapter_rejects_delay_operations() {
        let device = Mutex::<CriticalSectionRawMutex, _>::new(Recorder::new());
        let bus = SpiMock::new(CAPABILITIES, [Some(&device), None]);
        let mut handle = SpiDeviceHandle::new(&bus, ChipSelect::new(0), config());
        let mut buffer = [0u8; 1];
        assert_eq!(
            embedded_hal::spi::SpiDevice::transaction(
                &mut handle,
                &mut [Operation::Read(&mut buffer), Operation::DelayNs(10)],
            ),
            Err(SpiError::Unsupported)
        );
        // The read before the delay ran with chip select asserted, and chip select was released.
        let device = device.try_lock().unwrap();
        assert_eq!((device.asserts, device.deasserts), (1, 1));
    }
}
