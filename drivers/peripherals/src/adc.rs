//! ADC device nodes under `/devices/adc/<name>`.
//!
//! [`AdcHandle`] adapts a `peripherals::adc::AdcController` to
//! [`PeripheralHandle`]. A plain read performs one conversion of the selected
//! channel and returns its raw sample as a little-endian `u16` (two bytes).
//! Writes are unsupported.
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

use file_system::{ControlCommand, ControlCommandIdentifier, Error, Result};
use peripherals::adc::{AdcController, Error as AdcError, raw_to_millivolts};
use shared::AnyByLayout;

use crate::PeripheralHandle;

pub use device::adc::{
    ABI_VERSION, GET_INFO, InfoAbi, MillivoltsAbi, READ_MILLIVOLTS, SELECT_CHANNEL,
    SelectChannelAbi, VersionAbi,
};

/// Adapts an ADC converter to [`PeripheralHandle`]. The selected channel starts at 0.
pub struct AdcHandle<C> {
    controller: C,
    channel: u8,
}

impl<C: AdcController> AdcHandle<C> {
    pub fn new(controller: C) -> Self {
        Self {
            controller,
            channel: 0,
        }
    }

    fn info(&self) -> InfoAbi {
        let reference = self.controller.reference_millivolts();
        InfoAbi {
            version: ABI_VERSION,
            channels: self.controller.channel_count(),
            resolution_bits: self.controller.resolution_bits(),
            reference_millivolts: reference.unwrap_or(0),
            reference_present: u8::from(reference.is_some()),
            reserved: [0; 3],
        }
    }
}

impl<C> PeripheralHandle for AdcHandle<C>
where
    C: AdcController + Send,
{
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if buffer.len() < 2 {
            return Err(Error::InvalidParameter);
        }
        let raw = self.controller.read_raw(self.channel).map_err(map_error)?;
        buffer[..2].copy_from_slice(&raw.to_le_bytes());
        Ok(2)
    }

    fn write(&mut self, _buffer: &[u8]) -> Result<usize> {
        Err(Error::UnsupportedOperation)
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == GET_INFO::IDENTIFIER {
            check_version(GET_INFO::cast_input(input)?)?;
            *GET_INFO::cast_output(output)? = self.info();
        } else if command == SELECT_CHANNEL::IDENTIFIER {
            let request = SELECT_CHANNEL::cast_input(input)?;
            check_abi(request.version, request.reserved == 0)?;
            if request.channel >= self.controller.channel_count() {
                return Err(Error::InvalidParameter);
            }
            self.channel = request.channel;
            *SELECT_CHANNEL::cast_output(output)? = version_abi();
        } else if command == READ_MILLIVOLTS::IDENTIFIER {
            check_version(READ_MILLIVOLTS::cast_input(input)?)?;
            let reference = self
                .controller
                .reference_millivolts()
                .ok_or(Error::UnsupportedOperation)?;
            let raw = self.controller.read_raw(self.channel).map_err(map_error)?;
            let millivolts = raw_to_millivolts(raw, self.controller.resolution_bits(), reference);
            *READ_MILLIVOLTS::cast_output(output)? = MillivoltsAbi {
                version: ABI_VERSION,
                reserved: 0,
                millivolts,
            };
        } else {
            return Err(Error::UnsupportedOperation);
        }
        Ok(())
    }
}

fn version_abi() -> VersionAbi {
    VersionAbi {
        version: ABI_VERSION,
        reserved: 0,
    }
}

fn check_version(request: &VersionAbi) -> Result<()> {
    check_abi(request.version, request.reserved == 0)
}

fn check_abi(version: u16, reserved_is_zero: bool) -> Result<()> {
    if version == ABI_VERSION && reserved_is_zero {
        Ok(())
    } else {
        Err(Error::InvalidParameter)
    }
}

/// Maps an ADC error to the file system error seen by applications.
fn map_error(error: AdcError) -> Error {
    match error {
        AdcError::InvalidChannel => Error::InvalidParameter,
        AdcError::Busy => Error::RessourceBusy,
        AdcError::Unsupported => Error::UnsupportedOperation,
        AdcError::Other => Error::InputOutput,
    }
}

/// Mounts `controller` at `/devices/adc/<name>`, creating `/devices` and
/// `/devices/adc` when they do not exist. `name` must be a single path
/// component such as `adc0`.
#[cfg(feature = "vfs")]
pub async fn mount_adc<C>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    name: &str,
    controller: C,
) -> virtual_file_system::Result<()>
where
    C: AdcController + Send + 'static,
{
    use file_system::Path;

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(virtual_file_system::Error::FileSystem(Error::InvalidPath));
    }
    let path = Path::ADC_DEVICES
        .join(name)
        .ok_or(virtual_file_system::Error::FileSystem(Error::InvalidPath))?;
    let handle = AdcHandle::new(controller);

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::DEVICES)
            .await,
    )?;
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::ADC_DEVICES)
            .await,
    )?;

    crate::mount_peripheral(virtual_file_system, task, path, handle).await
}
