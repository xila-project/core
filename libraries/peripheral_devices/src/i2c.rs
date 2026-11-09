//! I2C device nodes under `/devices/i2c/<name>`.
//!
//! Plain file I/O:
//! - The node has one selected target address. `SELECT_ADDRESS` sets it. Until
//!   an address is selected, `read` and `write` fail with `NotInitialized`.
//! - `write` sends one write transfer containing the whole buffer to the
//!   selected address and returns `buffer.len()` on success.
//! - `read` receives `buffer.len()` bytes from the selected address.
//! - Each call is one non-blocking attempt. It opens the bus with the node's
//!   configured frequency, runs one transaction and drops the bus. A bus held
//!   by another handle returns `RessourceBusy`.
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
//!
//! Error mapping: `Busy` becomes `RessourceBusy`, a NACK on the address becomes
//! `NotFound`, other bus failures become `InputOutput`, and `Unsupported`
//! becomes `UnsupportedOperation`.

use embedded_hal::i2c::{Operation, SevenBitAddress, TenBitAddress};
use file_system::{ControlCommand, ControlCommandIdentifier, Error as FileSystemError, Result};
use peripherals::i2c::{Address, BusConfig, Error as BusError, I2cController, NoAcknowledge};
use shared::AnyByLayout;

use crate::PeripheralHandle;

#[cfg(feature = "vfs")]
use file_system::Path;
#[cfg(feature = "vfs")]
use task::TaskIdentifier;
#[cfg(feature = "vfs")]
use virtual_file_system::{Error, Result as VirtualFileSystemResult, VirtualFileSystem};

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

file_system::define_command!(SELECT_ADDRESS, Write, device::kind::I2C, 0, SelectAddressInput, ());
file_system::define_command!(GET_FREQUENCY, Read, device::kind::I2C, 1, (), FrequencyOutput);
file_system::define_command!(SET_FREQUENCY, Write, device::kind::I2C, 2, SetFrequencyInput, ());
file_system::define_command!(WRITE_READ, Write, device::kind::I2C, 3, WriteReadInput, WriteReadOutput);

/// Handle that exposes an I2C controller through [`PeripheralHandle`].
///
/// The handle owns the controller. Bus reservation happens per call, so the
/// handle never keeps a borrow of the controller between calls.
pub struct I2cHandle<C> {
    controller: C,
    frequency_hz: u32,
    address: Option<Address>,
}

impl<C: I2cController> I2cHandle<C> {
    /// Creates a handle with `config` as its bus frequency. Fails with
    /// `InvalidParameter` for a zero frequency and `UnsupportedOperation` for a
    /// frequency above the controller's maximum.
    pub fn new(controller: C, config: BusConfig) -> Result<Self> {
        let frequency_hz = validate_frequency(&controller.capabilities(), config.frequency_hz)?;
        Ok(Self {
            controller,
            frequency_hz,
            address: None,
        })
    }

    /// Opens the bus, runs `operations` against the selected address as one
    /// transaction and releases the bus.
    fn transfer(&mut self, operations: &mut [Operation<'_>]) -> Result<()> {
        let address = self.address.ok_or(FileSystemError::NotInitialized)?;
        let mut bus = self
            .controller
            .open(BusConfig::new(self.frequency_hz))
            .map_err(map_error)?;

        let result = match address {
            Address::SevenBit(address) => embedded_hal::i2c::I2c::<SevenBitAddress>::transaction(
                &mut bus, address, operations,
            ),
            Address::TenBit(address) => {
                embedded_hal::i2c::I2c::<TenBitAddress>::transaction(&mut bus, address, operations)
            }
        };
        result.map_err(map_error)
    }

    fn select_address(&mut self, input: &SelectAddressInput) -> Result<()> {
        check_header(input.version, input.reserved as u16)?;
        let capabilities = self.controller.capabilities();
        let address = match input.ten_bit {
            0 => {
                let value =
                    u8::try_from(input.address).map_err(|_| FileSystemError::InvalidParameter)?;
                Address::seven_bit(value).ok_or(FileSystemError::InvalidParameter)?
            }
            1 => {
                if !capabilities.ten_bit {
                    return Err(FileSystemError::UnsupportedOperation);
                }
                Address::ten_bit(input.address).ok_or(FileSystemError::InvalidParameter)?
            }
            _ => return Err(FileSystemError::InvalidParameter),
        };
        if !capabilities.supports_address(address) {
            return Err(FileSystemError::UnsupportedOperation);
        }
        self.address = Some(address);
        Ok(())
    }

    fn write_read(&mut self, input: &WriteReadInput, output: &mut WriteReadOutput) -> Result<()> {
        check_header(input.version, 0)?;
        let write_length = usize::from(input.write_length);
        let read_length = usize::from(input.read_length);
        if write_length > MAX_TRANSFER_LENGTH
            || read_length > MAX_TRANSFER_LENGTH
            || (write_length == 0 && read_length == 0)
        {
            return Err(FileSystemError::InvalidParameter);
        }

        output.version = ABI_VERSION;
        output.read_length = 0;
        output.reserved = 0;

        let write = &input.write[..write_length];
        let read = &mut output.read[..read_length];
        match (write_length, read_length) {
            (_, 0) => self.transfer(&mut [Operation::Write(write)])?,
            (0, _) => self.transfer(&mut [Operation::Read(read)])?,
            _ => self.transfer(&mut [Operation::Write(write), Operation::Read(read)])?,
        }
        output.read_length = read_length as u8;
        Ok(())
    }

    fn set_frequency(&mut self, input: &SetFrequencyInput) -> Result<()> {
        check_header(input.version, input.reserved)?;
        let capabilities = self.controller.capabilities();
        self.frequency_hz = validate_frequency(&capabilities, input.frequency_hz)?;
        Ok(())
    }
}

/// Rejects a version other than [`ABI_VERSION`] or a non-zero reserved field.
fn check_header(version: u16, reserved: u16) -> Result<()> {
    if version != ABI_VERSION || reserved != 0 {
        return Err(FileSystemError::InvalidParameter);
    }
    Ok(())
}

/// Validates a frequency against the controller capabilities.
fn validate_frequency(
    capabilities: &peripherals::i2c::Capabilities,
    frequency_hz: u32,
) -> Result<u32> {
    if frequency_hz == 0 {
        return Err(FileSystemError::InvalidParameter);
    }
    if !capabilities.supports_config(BusConfig::new(frequency_hz)) {
        return Err(FileSystemError::UnsupportedOperation);
    }
    Ok(frequency_hz)
}

fn map_error(error: BusError) -> FileSystemError {
    match error {
        BusError::Busy => FileSystemError::RessourceBusy,
        BusError::NoAcknowledge(NoAcknowledge::Address) => FileSystemError::NotFound,
        BusError::NoAcknowledge(_)
        | BusError::ArbitrationLoss
        | BusError::Timeout
        | BusError::Bus => FileSystemError::InputOutput,
        BusError::Unsupported => FileSystemError::UnsupportedOperation,
        BusError::Other => FileSystemError::Other,
    }
}

impl<C> PeripheralHandle for I2cHandle<C>
where
    C: I2cController + Send,
{
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let length = buffer.len();
        self.transfer(&mut [Operation::Read(buffer)])?;
        Ok(length)
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.transfer(&mut [Operation::Write(buffer)])?;
        Ok(buffer.len())
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        match command {
            SELECT_ADDRESS::IDENTIFIER => {
                let input = SELECT_ADDRESS::cast_input(input)?;
                self.select_address(input)
            }
            GET_FREQUENCY::IDENTIFIER => {
                let output = GET_FREQUENCY::cast_output(output)?;
                let capabilities = self.controller.capabilities();
                *output = FrequencyOutput {
                    version: ABI_VERSION,
                    ten_bit_supported: u8::from(capabilities.ten_bit),
                    reserved: 0,
                    frequency_hz: self.frequency_hz,
                    max_frequency_hz: capabilities.max_frequency_hz,
                };
                Ok(())
            }
            SET_FREQUENCY::IDENTIFIER => {
                let input = SET_FREQUENCY::cast_input(input)?;
                self.set_frequency(input)
            }
            WRITE_READ::IDENTIFIER => {
                let (input, output) = WRITE_READ::cast(input, output)?;
                self.write_read(input, output)
            }
            _ => Err(FileSystemError::UnsupportedOperation),
        }
    }
}

/// Creates `/devices/i2c` if needed, then mounts `controller` at
/// `/devices/i2c/<name>` with `config` as its bus frequency.
///
/// `name` must be non-empty and use only ASCII letters, digits, `_` or `-`;
/// otherwise the call fails with `InvalidPath`. The caller chooses the
/// controller, so this helper adds no permission policy beyond the VFS
/// mount defaults.
#[cfg(feature = "vfs")]
pub async fn mount_i2c_device<C>(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    name: &str,
    controller: C,
    config: BusConfig,
) -> VirtualFileSystemResult<()>
where
    C: I2cController + Send + 'static,
{
    let valid_name = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if !valid_name {
        return Err(FileSystemError::InvalidPath.into());
    }

    ensure_directory(virtual_file_system, task, Path::DEVICES).await?;
    ensure_directory(virtual_file_system, task, Path::I2C_DEVICES).await?;

    let path = Path::I2C_DEVICES
        .join(name)
        .ok_or(FileSystemError::InvalidPath)?;
    let handle = I2cHandle::new(controller, config)?;
    crate::mount_peripheral(virtual_file_system, task, path, handle).await
}

#[cfg(feature = "vfs")]
async fn ensure_directory(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: &Path,
) -> VirtualFileSystemResult<()> {
    match virtual_file_system.create_directory(task, &path).await {
        Ok(()) => Ok(()),
        Err(Error::AlreadyExists)
        | Err(Error::FileSystem(
            FileSystemError::AlreadyExists | FileSystemError::DirectoryAlreadyExists,
        )) => Ok(()),
        Err(error) => Err(error),
    }
}
