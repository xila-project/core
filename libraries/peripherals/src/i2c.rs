//! I2C controller contract.
//!
//! Drivers use `embedded_hal::i2c::I2c` (and `embedded_hal_async::i2c::I2c` where the bus
//! provides it) on the handle returned by `I2cController::open`. Xila owns only what
//! embedded-hal does not express: bus reservation, validated addresses, configuration,
//! capability reporting and the error mapping of the reservation step.

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::{I2cSlave, MockI2cBus, MockI2cController};

use embedded_hal::i2c::{SevenBitAddress, TenBitAddress};

const SEVEN_BIT_MAX: u8 = 0x7F;
const TEN_BIT_MAX: u16 = 0x3FF;

/// Target address, validated at construction. Used as the key of a bus endpoint registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Address {
    SevenBit(SevenBitAddress),
    TenBit(TenBitAddress),
}

impl Address {
    pub const fn seven_bit(value: SevenBitAddress) -> Option<Self> {
        if value <= SEVEN_BIT_MAX {
            Some(Self::SevenBit(value))
        } else {
            None
        }
    }

    pub const fn ten_bit(value: TenBitAddress) -> Option<Self> {
        if value <= TEN_BIT_MAX {
            Some(Self::TenBit(value))
        } else {
            None
        }
    }
}

/// Bus configuration applied when the bus is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusConfig {
    pub frequency_hz: u32,
}

impl BusConfig {
    pub const fn new(frequency_hz: u32) -> Self {
        Self { frequency_hz }
    }
}

/// Static capabilities of a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub ten_bit: bool,
    pub max_frequency_hz: u32,
}

impl Capabilities {
    pub const fn supports_address(self, address: Address) -> bool {
        match address {
            Address::SevenBit(_) => true,
            Address::TenBit(_) => self.ten_bit,
        }
    }

    pub const fn supports_config(self, config: BusConfig) -> bool {
        config.frequency_hz > 0 && config.frequency_hz <= self.max_frequency_hz
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoAcknowledge {
    Address,
    Data,
    Unknown,
}

/// I2C error. Implements `embedded_hal::i2c::Error`, so drivers see only embedded-hal kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NoAcknowledge(NoAcknowledge),
    ArbitrationLoss,
    Timeout,
    Bus,
    Busy,
    Unsupported,
    Other,
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        use embedded_hal::i2c::{ErrorKind, NoAcknowledgeSource};
        match self {
            Self::NoAcknowledge(NoAcknowledge::Address) => {
                ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address)
            }
            Self::NoAcknowledge(NoAcknowledge::Data) => {
                ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data)
            }
            Self::NoAcknowledge(NoAcknowledge::Unknown) => {
                ErrorKind::NoAcknowledge(NoAcknowledgeSource::Unknown)
            }
            Self::ArbitrationLoss => ErrorKind::ArbitrationLoss,
            Self::Bus => ErrorKind::Bus,
            // embedded-hal has no timeout, busy or unsupported kinds.
            Self::Timeout | Self::Busy | Self::Unsupported | Self::Other => ErrorKind::Other,
        }
    }
}

/// Controller that hands out exclusive bus handles.
///
/// The handle implements the embedded-hal blocking `I2c` for both address widths. Its
/// `transaction` runs all operations atomically with repeated START between them. A handle
/// reserves the bus until it is dropped, so a second transaction cannot interleave.
pub trait I2cController {
    type Bus<'a>: embedded_hal::i2c::I2c<SevenBitAddress, Error = Error>
        + embedded_hal::i2c::I2c<TenBitAddress, Error = Error>
    where
        Self: 'a;

    fn capabilities(&self) -> Capabilities;

    /// Reserves the bus until the returned handle is dropped.
    /// Fails with `Error::Unsupported` for an unsupported frequency and `Error::Busy` if
    /// another handle is alive.
    fn open(&self, config: BusConfig) -> Result<Self::Bus<'_>, Error>;
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use core::task::{Context, Poll, Waker};

    use embedded_hal::i2c::{Error as _, I2c, NoAcknowledgeSource, Operation};

    use super::{Address, BusConfig, Capabilities, Error, I2cController, I2cSlave};
    use super::{MockI2cController, NoAcknowledge};

    const CAPABILITIES: Capabilities = Capabilities {
        ten_bit: false,
        max_frequency_hz: 400_000,
    };

    /// Register-file slave: the first written byte selects the register pointer,
    /// later written bytes are stored from that pointer on, reads return from it.
    struct RegisterSlave {
        registers: [u8; 8],
        pointer: usize,
    }

    impl RegisterSlave {
        const fn new(registers: [u8; 8]) -> Self {
            Self {
                registers,
                pointer: 0,
            }
        }
    }

    impl I2cSlave for RegisterSlave {
        fn write(&mut self, data: &[u8]) -> bool {
            let mut rest = data;
            if let Some((&pointer, values)) = rest.split_first() {
                self.pointer = usize::from(pointer) % self.registers.len();
                rest = values;
            }
            for &value in rest {
                self.registers[self.pointer] = value;
                self.pointer = (self.pointer + 1) % self.registers.len();
            }
            true
        }

        fn read(&mut self, buffer: &mut [u8]) {
            for byte in buffer {
                *byte = self.registers[self.pointer];
                self.pointer = (self.pointer + 1) % self.registers.len();
            }
        }
    }

    struct NackingSlave;

    impl I2cSlave for NackingSlave {
        fn write(&mut self, _data: &[u8]) -> bool {
            false
        }

        fn read(&mut self, buffer: &mut [u8]) {
            buffer.fill(0xEE);
        }
    }

    fn address(value: u8) -> u8 {
        value
    }

    /// Mock futures complete on first poll, so one poll with a no-op waker suffices.
    fn block_on<F: core::future::Future>(future: F) -> F::Output {
        let mut context = Context::from_waker(Waker::noop());
        let future = core::pin::pin!(future);
        match future.poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("mock future must complete immediately"),
        }
    }

    #[test]
    fn address_constructors_validate_range() {
        assert_eq!(Address::seven_bit(0x7F), Some(Address::SevenBit(0x7F)));
        assert_eq!(Address::seven_bit(0x80), None);
        assert_eq!(Address::ten_bit(0x3FF), Some(Address::TenBit(0x3FF)));
        assert_eq!(Address::ten_bit(0x400), None);
    }

    #[test]
    fn simple_write_reaches_slave() {
        let mut slave = RegisterSlave::new([0; 8]);
        let controller = MockI2cController::<2>::new(CAPABILITIES);
        controller
            .attach(Address::SevenBit(0x42), &mut slave)
            .unwrap();
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        i2c.write(address(0x42), &[0x02, 0xAA, 0xBB]).unwrap();
        drop(i2c);
        assert_eq!(slave.registers[2..4], [0xAA, 0xBB]);
    }

    #[test]
    fn simple_read_returns_slave_data() {
        let mut slave = RegisterSlave::new([1, 2, 3, 4, 5, 6, 7, 8]);
        let controller = MockI2cController::<2>::new(CAPABILITIES);
        controller
            .attach(Address::SevenBit(0x42), &mut slave)
            .unwrap();
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let mut buffer = [0u8; 3];
        i2c.read(address(0x42), &mut buffer).unwrap();
        assert_eq!(buffer, [1, 2, 3]);
    }

    #[test]
    fn write_then_read_uses_one_transaction_with_repeated_start() {
        // Register-read pattern: write register pointer 7, then read 2 bytes.
        let mut slave = RegisterSlave::new([0, 0, 0, 0, 0, 0, 0, 0x11]);
        let controller = MockI2cController::<2>::new(CAPABILITIES);
        controller
            .attach(Address::SevenBit(0x76), &mut slave)
            .unwrap();
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let mut buffer = [0u8; 2];
        i2c.write_read(address(0x76), &[7], &mut buffer).unwrap();
        assert_eq!(buffer, [0x11, 0x00]);
        drop(i2c);
        assert_eq!(slave.pointer, 1);
    }

    #[test]
    fn transaction_runs_operations_in_order() {
        let mut slave = RegisterSlave::new([0; 8]);
        let controller = MockI2cController::<2>::new(CAPABILITIES);
        controller
            .attach(Address::SevenBit(0x42), &mut slave)
            .unwrap();
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let mut buffer = [0u8; 1];
        i2c.transaction(
            address(0x42),
            &mut [
                Operation::Write(&[3, 0x5A]),
                Operation::Write(&[4, 0x5B]),
                Operation::Read(&mut buffer),
            ],
        )
        .unwrap();
        drop(i2c);
        assert_eq!(slave.registers[3..5], [0x5A, 0x5B]);
        assert_eq!(buffer, [0x00]);
    }

    #[test]
    fn async_write_read_matches_blocking_path() {
        let mut slave = RegisterSlave::new([9, 8, 7, 6, 5, 4, 3, 2]);
        let controller = MockI2cController::<2>::new(CAPABILITIES);
        controller
            .attach(Address::SevenBit(0x42), &mut slave)
            .unwrap();
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let mut buffer = [0u8; 2];
        block_on(embedded_hal_async::i2c::I2c::write_read(
            &mut i2c,
            address(0x42),
            &[1],
            &mut buffer,
        ))
        .unwrap();
        assert_eq!(buffer, [8, 7]);
    }

    #[test]
    fn absent_address_is_nacked_on_address() {
        let controller = MockI2cController::<1>::new(CAPABILITIES);
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let error = i2c.write(address(0x50), &[0]).unwrap_err();
        assert_eq!(
            error.kind(),
            embedded_hal::i2c::ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address)
        );
    }

    #[test]
    fn data_nack_is_distinct_from_address_nack() {
        let mut slave = NackingSlave;
        let controller = MockI2cController::<2>::new(CAPABILITIES);
        controller
            .attach(Address::SevenBit(0x42), &mut slave)
            .unwrap();
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let error = i2c.write(address(0x42), &[1]).unwrap_err();
        assert_eq!(error, Error::NoAcknowledge(NoAcknowledge::Data));
    }

    #[test]
    fn unsupported_ten_bit_address_is_rejected() {
        let controller = MockI2cController::<1>::new(CAPABILITIES);
        let mut i2c = controller.open(BusConfig::new(100_000)).unwrap();
        let error = embedded_hal::i2c::I2c::<embedded_hal::i2c::TenBitAddress>::transaction(
            &mut i2c,
            0x123,
            &mut [Operation::Write(&[0])],
        )
        .unwrap_err();
        assert_eq!(error, Error::Unsupported);
    }

    #[test]
    fn unsupported_frequency_is_rejected_at_open() {
        let controller = MockI2cController::<1>::new(CAPABILITIES);
        assert!(matches!(
            controller.open(BusConfig::new(1_000_000)),
            Err(Error::Unsupported)
        ));
        assert!(matches!(
            controller.open(BusConfig::new(0)),
            Err(Error::Unsupported)
        ));
    }

    #[test]
    fn bus_is_exclusive_until_handle_is_dropped() {
        let controller = MockI2cController::<1>::new(CAPABILITIES);
        let first = controller.open(BusConfig::new(100_000)).unwrap();
        assert!(matches!(
            controller.open(BusConfig::new(100_000)),
            Err(Error::Busy)
        ));
        drop(first);
        assert!(controller.open(BusConfig::new(100_000)).is_ok());
    }

    #[test]
    fn error_kinds_map_to_embedded_hal() {
        use embedded_hal::i2c::ErrorKind;
        assert_eq!(Error::ArbitrationLoss.kind(), ErrorKind::ArbitrationLoss);
        assert_eq!(Error::Bus.kind(), ErrorKind::Bus);
        assert_eq!(Error::Timeout.kind(), ErrorKind::Other);
        assert_eq!(Error::Busy.kind(), ErrorKind::Other);
        assert_eq!(Error::Unsupported.kind(), ErrorKind::Other);
        assert_eq!(Error::Other.kind(), ErrorKind::Other);
        assert_eq!(
            Error::NoAcknowledge(NoAcknowledge::Unknown).kind(),
            ErrorKind::NoAcknowledge(NoAcknowledgeSource::Unknown)
        );
    }

    #[test]
    fn bus_handle_fits_in_two_machine_words() {
        assert!(core::mem::size_of::<super::MockI2cBus<'_, '_, 2>>() <= 2 * size_of::<usize>());
    }
}
