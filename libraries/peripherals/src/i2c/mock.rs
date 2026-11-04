use core::sync::atomic::{AtomicBool, Ordering};

use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use embedded_hal::i2c::{ErrorType, Operation, SevenBitAddress, TenBitAddress};

use super::{Address, BusConfig, Capabilities, Error, I2cController, NoAcknowledge};

/// Simulated I2C target. Returns `false` from `write` to NACK the data bytes. Slaves are `Send`
/// so the controller can sit behind a mutex.
pub trait I2cSlave: Send {
    fn write(&mut self, data: &[u8]) -> bool;

    fn read(&mut self, buffer: &mut [u8]);
}

struct Attachment<'s> {
    address: Address,
    slave: &'s mut dyn I2cSlave,
}

/// Virtual I2C controller with up to `N` attached slaves. Slaves are borrowed, not owned,
/// so tests can inspect them after the bus is dropped.
pub struct MockI2cController<'s, const N: usize> {
    capabilities: Capabilities,
    slaves: Mutex<CriticalSectionRawMutex, [Option<Attachment<'s>>; N]>,
    busy: AtomicBool,
}

impl<'s, const N: usize> MockI2cController<'s, N> {
    fn slaves(&self) -> MutexGuard<'_, CriticalSectionRawMutex, [Option<Attachment<'s>>; N]> {
        self.slaves
            .try_lock()
            .expect("I2C mock slaves are already locked")
    }

    pub const fn new(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            slaves: Mutex::new([const { None }; N]),
            busy: AtomicBool::new(false),
        }
    }

    /// Attaches `slave` at `address`. Fails with `Unsupported` for an address the
    /// capabilities do not allow, and with `Other` if the table is full or the address is taken.
    pub fn attach(&self, address: Address, slave: &'s mut dyn I2cSlave) -> Result<(), Error> {
        if !self.capabilities.supports_address(address) {
            return Err(Error::Unsupported);
        }
        let mut slaves = self.slaves();
        if slaves
            .iter()
            .flatten()
            .any(|attachment| attachment.address == address)
        {
            return Err(Error::Other);
        }
        let free = slaves
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or(Error::Other)?;
        *free = Some(Attachment { address, slave });
        Ok(())
    }
}

impl<'s, const N: usize> I2cController for MockI2cController<'s, N> {
    type Bus<'a>
        = MockI2cBus<'a, 's, N>
    where
        Self: 'a;

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn open(&self, config: BusConfig) -> Result<Self::Bus<'_>, Error> {
        if !self.capabilities.supports_config(config) {
            return Err(Error::Unsupported);
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(Error::Busy);
        }
        Ok(MockI2cBus { controller: self })
    }
}

/// Exclusive handle on a `MockI2cController`; releases the bus on drop.
///
/// Implements the embedded-hal blocking and async `I2c` traits directly, so third-party
/// drivers run on it unchanged.
pub struct MockI2cBus<'a, 's, const N: usize> {
    controller: &'a MockI2cController<'s, N>,
}

impl<const N: usize> MockI2cBus<'_, '_, N> {
    fn run(&mut self, address: Address, operations: &mut [Operation<'_>]) -> Result<(), Error> {
        if !self.controller.capabilities.supports_address(address) {
            return Err(Error::Unsupported);
        }
        let mut slaves = self.controller.slaves();
        // Address phase: an absent target NACKs before any operation is sent.
        let attachment = slaves
            .iter_mut()
            .flatten()
            .find(|attachment| attachment.address == address)
            .ok_or(Error::NoAcknowledge(NoAcknowledge::Address))?;
        for operation in operations.iter_mut() {
            match operation {
                Operation::Write(data) => {
                    if !attachment.slave.write(data) {
                        return Err(Error::NoAcknowledge(NoAcknowledge::Data));
                    }
                }
                Operation::Read(buffer) => attachment.slave.read(buffer),
            }
        }
        Ok(())
    }
}

impl<const N: usize> ErrorType for MockI2cBus<'_, '_, N> {
    type Error = Error;
}

impl<const N: usize> embedded_hal::i2c::I2c<SevenBitAddress> for MockI2cBus<'_, '_, N> {
    fn transaction(
        &mut self,
        address: SevenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        let address = Address::seven_bit(address).ok_or(Error::Unsupported)?;
        self.run(address, operations)
    }
}

impl<const N: usize> embedded_hal::i2c::I2c<TenBitAddress> for MockI2cBus<'_, '_, N> {
    fn transaction(
        &mut self,
        address: TenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        let address = Address::ten_bit(address).ok_or(Error::Unsupported)?;
        self.run(address, operations)
    }
}

// The mock completes synchronously, so the async path runs the blocking one.
impl<const N: usize> embedded_hal_async::i2c::I2c<SevenBitAddress> for MockI2cBus<'_, '_, N> {
    async fn transaction(
        &mut self,
        address: SevenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        embedded_hal::i2c::I2c::transaction(self, address, operations)
    }
}

impl<const N: usize> embedded_hal_async::i2c::I2c<TenBitAddress> for MockI2cBus<'_, '_, N> {
    async fn transaction(
        &mut self,
        address: TenBitAddress,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Error> {
        embedded_hal::i2c::I2c::<TenBitAddress>::transaction(self, address, operations)
    }
}

impl<const N: usize> Drop for MockI2cBus<'_, '_, N> {
    fn drop(&mut self) {
        self.controller.busy.store(false, Ordering::Release);
    }
}
