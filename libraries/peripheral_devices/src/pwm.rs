//! PWM device nodes under `/devices/pwm/<name>`.
//!
//! [`PwmHandle`] holds one opened channel of a `peripherals::pwm::PwmController`
//! and adapts it to [`PeripheralHandle`]. Plain `File` access carries the duty
//! cycle as a little-endian `u16` (2 bytes per write, 2 bytes per read); each
//! write applies the duty immediately. The period and percentage-style duty are
//! set through the control commands below, which are the only PWM-specific ABI.
//!
//! # Control commands (kind `b'p'`)
//!
//! | Number | Command            | Direction | Input              | Output        | Effect                                        |
//! |--------|--------------------|-----------|--------------------|---------------|-----------------------------------------------|
//! | `0x00` | `GET_INFO`         | Read      | `PwmVersionAbi`    | `PwmInfoAbi`  | Returns channel count, max duty and period.   |
//! | `0x01` | `SET_FREQUENCY`    | Write     | `PwmFrequencyAbi`  | `PwmInfoAbi`  | Sets the channel period in hertz.             |
//! | `0x02` | `SET_DUTY_PERMILLE`| Write     | `PwmDutyPermilleAbi`| `PwmInfoAbi` | Sets duty as 0..=1000 per mille of max duty.  |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and
//! carries zeroed `reserved` fields. A wrong version, a non-zero reserved field
//! or an out-of-range value is `InvalidParameter`. A frequency the controller
//! cannot express, or a fixed-period controller, is `UnsupportedOperation`.

use embedded_hal::pwm::SetDutyCycle;
use file_system::{ControlCommand, ControlCommandIdentifier, Error, Result, define_command};
use peripherals::pwm::{ChannelId, Error as PwmError, PwmController};
use shared::AnyByLayout;

use crate::PeripheralHandle;

/// Version of every PWM control ABI struct in this module.
pub const ABI_VERSION: u16 = 1;

/// Largest value accepted by [`SET_DUTY_PERMILLE`].
pub const PERMILLE_FULL_SCALE: u16 = 1_000;

/// Carries only the ABI version and a reserved field. Input of [`GET_INFO`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmVersionAbi {
    pub version: u16,
    pub reserved: u16,
}

/// Channel description returned by every PWM command.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmInfoAbi {
    pub version: u16,
    /// Number of channels on the controller.
    pub channel_count: u16,
    /// Duty value that means 100 percent on this channel.
    pub max_duty_cycle: u16,
    pub reserved: u16,
    /// Current period of this channel in hertz.
    pub frequency_hz: u32,
}

/// Input of [`SET_FREQUENCY`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmFrequencyAbi {
    pub version: u16,
    pub reserved: u16,
    pub frequency_hz: u32,
}

/// Input of [`SET_DUTY_PERMILLE`]. `permille` is 0..=[`PERMILLE_FULL_SCALE`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmDutyPermilleAbi {
    pub version: u16,
    pub permille: u16,
}

define_command!(GET_INFO, Read, device::kind::PWM, 0x00, PwmVersionAbi, PwmInfoAbi);
define_command!(
    SET_FREQUENCY,
    Write,
    device::kind::PWM,
    0x01,
    PwmFrequencyAbi,
    PwmInfoAbi
);
define_command!(
    SET_DUTY_PERMILLE,
    Write,
    device::kind::PWM,
    0x02,
    PwmDutyPermilleAbi,
    PwmInfoAbi
);

/// One opened PWM channel adapted to [`PeripheralHandle`].
///
/// The channel stays reserved for the life of the handle. `duty` records the
/// last value written through this handle, which is what plain reads return;
/// it starts at zero and the hardware is not touched when the handle is created.
pub struct PwmHandle<'a, C: PwmController> {
    controller: &'a C,
    id: ChannelId,
    channel: C::Channel<'a>,
    duty: u16,
}

impl<'a, C: PwmController> PwmHandle<'a, C> {
    /// Opens `id` on `controller`. Fails with `RessourceBusy` if the channel is
    /// already held and with `InvalidParameter` if it does not exist.
    pub fn new(controller: &'a C, id: ChannelId) -> Result<Self> {
        let channel = controller.open(id).map_err(map_error)?;
        Ok(Self {
            controller,
            id,
            channel,
            duty: 0,
        })
    }

    /// Validates `duty` against this channel's maximum and applies it.
    fn set_duty(&mut self, duty: u16) -> Result<()> {
        if duty > self.channel.max_duty_cycle() {
            return Err(Error::InvalidParameter);
        }
        self.channel.set_duty_cycle(duty).map_err(map_error)?;
        self.duty = duty;
        Ok(())
    }

    fn info(&self) -> Result<PwmInfoAbi> {
        Ok(PwmInfoAbi {
            version: ABI_VERSION,
            channel_count: u16::from(self.controller.capabilities().channel_count),
            max_duty_cycle: self.channel.max_duty_cycle(),
            reserved: 0,
            frequency_hz: self.controller.frequency_hz(self.id).map_err(map_error)?,
        })
    }
}

impl<'a, C> PeripheralHandle for PwmHandle<'a, C>
where
    C: PwmController + Sync,
    C::Channel<'a>: Send,
{
    /// Copies the last duty written through this handle as little-endian `u16`.
    /// The buffer must hold at least two bytes.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let bytes = buffer.get_mut(..2).ok_or(Error::InvalidParameter)?;
        bytes.copy_from_slice(&self.duty.to_le_bytes());
        Ok(2)
    }

    /// Applies one little-endian `u16` duty. Partial writes are refused: the
    /// buffer must be exactly two bytes, so the returned count is always two.
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        let bytes: [u8; 2] = buffer.try_into().map_err(|_| Error::InvalidParameter)?;
        self.set_duty(u16::from_le_bytes(bytes))?;
        Ok(2)
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == GET_INFO::IDENTIFIER {
            let request = GET_INFO::cast_input(input)?;
            check_header(request.version, request.reserved)?;
            *GET_INFO::cast_output(output)? = self.info()?;
        } else if command == SET_FREQUENCY::IDENTIFIER {
            let request = SET_FREQUENCY::cast_input(input)?;
            check_header(request.version, request.reserved)?;
            self.controller
                .set_frequency(self.id, request.frequency_hz)
                .map_err(map_error)?;
            *SET_FREQUENCY::cast_output(output)? = self.info()?;
        } else if command == SET_DUTY_PERMILLE::IDENTIFIER {
            let request = SET_DUTY_PERMILLE::cast_input(input)?;
            if request.version != ABI_VERSION || request.permille > PERMILLE_FULL_SCALE {
                return Err(Error::InvalidParameter);
            }
            let max = self.channel.max_duty_cycle();
            let duty =
                u32::from(request.permille) * u32::from(max) / u32::from(PERMILLE_FULL_SCALE);
            // The quotient is at most `max`, which is a `u16`.
            self.set_duty(duty as u16)?;
            *SET_DUTY_PERMILLE::cast_output(output)? = self.info()?;
        } else {
            return Err(Error::UnsupportedOperation);
        }
        Ok(())
    }
}

fn check_header(version: u16, reserved: u16) -> Result<()> {
    if version == ABI_VERSION && reserved == 0 {
        Ok(())
    } else {
        Err(Error::InvalidParameter)
    }
}

/// Maps a PWM error to the file system error seen by applications.
fn map_error(error: PwmError) -> Error {
    match error {
        PwmError::InvalidChannel | PwmError::InvalidFrequency | PwmError::InvalidDuty => {
            Error::InvalidParameter
        }
        PwmError::Busy => Error::RessourceBusy,
        PwmError::Unsupported => Error::UnsupportedOperation,
        PwmError::Other => Error::InputOutput,
    }
}

/// Mounts `controller` channel `channel` at `/devices/pwm/<name>`, creating
/// `/devices` and `/devices/pwm` when they do not exist. `name` must be a single
/// path component such as `pwm0`. The channel is opened here, so a busy or
/// missing channel fails the mount.
#[cfg(feature = "vfs")]
pub async fn mount_pwm<C>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    name: &str,
    controller: &'static C,
    channel: ChannelId,
) -> virtual_file_system::Result<()>
where
    C: PwmController + Sync + 'static,
    C::Channel<'static>: Send,
{
    use file_system::Path;

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(virtual_file_system::Error::FileSystem(Error::InvalidPath));
    }
    let path = Path::PWM_DEVICES
        .join(name)
        .ok_or(virtual_file_system::Error::FileSystem(Error::InvalidPath))?;
    let handle =
        PwmHandle::new(controller, channel).map_err(virtual_file_system::Error::FileSystem)?;

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::DEVICES)
            .await,
    )?;
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::PWM_DEVICES)
            .await,
    )?;

    crate::mount_peripheral(virtual_file_system, task, path, handle).await
}
