//! GPIO line nodes under `/devices/gpio/<name>`.
//!
//! [`GpioHandle`] adapts one line of a `peripherals::gpio::GpioController` to
//! [`PeripheralHandle`]. A plain read returns the line level as one byte, `0` or
//! `1`. A plain write sets the level of an output line from one byte, `0` or
//! `1`; writes to an input line are unsupported. Reads and writes before the
//! line is configured fail with `NotInitialized`.
//!
//! # Control commands (kind `b'g'`)
//!
//! | Number | Command           | Direction | Input           | Output       | Effect                                              |
//! |--------|-------------------|-----------|-----------------|--------------|-----------------------------------------------------|
//! | `0x00` | `CONFIGURE`       | Write     | `ConfigureAbi`  | `VersionAbi` | Releases the line, then requests it as input/output. |
//! | `0x01` | `RELEASE`         | Write     | `VersionAbi`    | `VersionAbi` | Releases the line, returning it to high impedance.  |
//! | `0x02` | `GET_CAPABILITIES`| Read      | `VersionAbi`    | `CapabilitiesAbi` | Reports what the line supports.                  |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and its
//! reserved fields must be zero; otherwise the command fails with
//! `InvalidParameter`. Field values outside their documented range are
//! `InvalidParameter`. A controller refusal to take the line (unsupported
//! option, line already in use) is reported as `InputOutput`.

use file_system::{ControlCommand, ControlCommandIdentifier, Error, Result, define_command};
use peripherals::gpio::{
    DriveMode, GpioController, InputConfig, LineCapabilities, LineId, OutputConfig, PinState, Pull,
};
use shared::AnyByLayout;

use crate::PeripheralHandle;

/// Version of every GPIO control ABI struct in this module.
pub const ABI_VERSION: u16 = 1;

/// Carries only the ABI version. Used as input of commands without payload and
/// as output of commands that report success.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VersionAbi {
    pub version: u16,
    pub reserved: u16,
}

/// Requested configuration of the line.
///
/// - `direction`: `0` input, `1` output.
/// - `pull`: `0` none, `1` pull up, `2` pull down.
/// - `drive_mode`: `0` push-pull, `1` open-drain. Outputs only.
/// - `initial_level`: `0` low, `1` high. Outputs only.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConfigureAbi {
    pub version: u16,
    pub direction: u8,
    pub pull: u8,
    pub drive_mode: u8,
    pub initial_level: u8,
    pub reserved: u16,
}

/// Line capabilities; each flag is `0` or `1`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapabilitiesAbi {
    pub version: u16,
    pub input: u8,
    pub output: u8,
    pub pull_up: u8,
    pub pull_down: u8,
    pub open_drain: u8,
    pub reserved: u8,
}

define_command!(CONFIGURE, Write, device::kind::GPIO, 0x00, ConfigureAbi, VersionAbi);
define_command!(RELEASE, Write, device::kind::GPIO, 0x01, VersionAbi, VersionAbi);
define_command!(
    GET_CAPABILITIES,
    Read,
    device::kind::GPIO,
    0x02,
    VersionAbi,
    CapabilitiesAbi
);

/// Owned line handle. Dropping a configured handle releases the line through
/// the controller's own handle drop.
enum Pin<C: GpioController + 'static> {
    Input(C::Input<'static>),
    Output(C::Output<'static>),
}

/// Adapts one GPIO line to [`PeripheralHandle`]. The line starts unconfigured.
pub struct GpioHandle<C: GpioController + 'static> {
    controller: &'static C,
    line: LineId,
    pin: Option<Pin<C>>,
}

impl<C: GpioController + Sync + 'static> GpioHandle<C> {
    pub const fn new(controller: &'static C, line: LineId) -> Self {
        Self {
            controller,
            line,
            pin: None,
        }
    }

    fn configure(&mut self, request: &ConfigureAbi) -> Result<()> {
        check_abi(request.version, request.reserved == 0)?;

        let pull = match request.pull {
            0 => Pull::Disabled,
            1 => Pull::Up,
            2 => Pull::Down,
            _ => return Err(Error::InvalidParameter),
        };

        // Release the current line before requesting it again, or the
        // controller would refuse the new request as already in use.
        self.pin = None;

        self.pin = Some(match request.direction {
            0 => Pin::Input(
                self.controller
                    .request_input(self.line, InputConfig::new(pull))
                    .map_err(|_| Error::InputOutput)?,
            ),
            1 => {
                let drive_mode = match request.drive_mode {
                    0 => DriveMode::PushPull,
                    1 => DriveMode::OpenDrain,
                    _ => return Err(Error::InvalidParameter),
                };
                let initial_state = match request.initial_level {
                    0 => PinState::Low,
                    1 => PinState::High,
                    _ => return Err(Error::InvalidParameter),
                };

                Pin::Output(
                    self.controller
                        .request_output(
                            self.line,
                            OutputConfig::new(pull, drive_mode, initial_state),
                        )
                        .map_err(|_| Error::InputOutput)?,
                )
            }
            _ => return Err(Error::InvalidParameter),
        });

        Ok(())
    }

    fn capabilities(&self) -> Result<CapabilitiesAbi> {
        let capabilities: LineCapabilities = self
            .controller
            .capabilities(self.line)
            .ok_or(Error::NotFound)?;

        Ok(CapabilitiesAbi {
            version: ABI_VERSION,
            input: u8::from(capabilities.input),
            output: u8::from(capabilities.output),
            pull_up: u8::from(capabilities.pull_up),
            pull_down: u8::from(capabilities.pull_down),
            open_drain: u8::from(capabilities.open_drain),
            reserved: 0,
        })
    }
}

impl<C> PeripheralHandle for GpioHandle<C>
where
    C: GpioController + Sync + 'static,
    C::Input<'static>: Send,
    C::Output<'static>: Send,
{
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        use embedded_hal::digital::{InputPin, StatefulOutputPin};

        let first = buffer.first_mut().ok_or(Error::InvalidParameter)?;

        *first = match self.pin.as_mut().ok_or(Error::NotInitialized)? {
            Pin::Input(pin) => u8::from(pin.is_high().map_err(|_| Error::InputOutput)?),
            Pin::Output(pin) => u8::from(pin.is_set_high().map_err(|_| Error::InputOutput)?),
        };

        Ok(1)
    }

    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        use embedded_hal::digital::OutputPin;

        let level = match buffer.first().ok_or(Error::InvalidParameter)? {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidParameter),
        };

        match self.pin.as_mut().ok_or(Error::NotInitialized)? {
            Pin::Output(pin) => {
                let result = if level { pin.set_high() } else { pin.set_low() };
                result.map_err(|_| Error::InputOutput)?;
                Ok(1)
            }
            Pin::Input(_) => Err(Error::UnsupportedOperation),
        }
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == CONFIGURE::IDENTIFIER {
            self.configure(CONFIGURE::cast_input(input)?)?;
            *CONFIGURE::cast_output(output)? = version_abi();
        } else if command == RELEASE::IDENTIFIER {
            check_version(RELEASE::cast_input(input)?)?;
            self.pin = None;
            *RELEASE::cast_output(output)? = version_abi();
        } else if command == GET_CAPABILITIES::IDENTIFIER {
            check_version(GET_CAPABILITIES::cast_input(input)?)?;
            *GET_CAPABILITIES::cast_output(output)? = self.capabilities()?;
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

/// Mounts `controller` line `line` at `/devices/gpio/<name>`, creating
/// `/devices` and `/devices/gpio` when they do not exist. `name` must be a single
/// path component such as `pin4`.
#[cfg(feature = "vfs")]
pub async fn mount_gpio<C>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    name: &str,
    controller: &'static C,
    line: LineId,
) -> virtual_file_system::Result<()>
where
    C: GpioController + Sync + 'static,
    C::Input<'static>: Send,
    C::Output<'static>: Send,
{
    use file_system::Path;

    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(virtual_file_system::Error::FileSystem(Error::InvalidPath));
    }
    let path = Path::GPIO_DEVICES
        .join(name)
        .ok_or(virtual_file_system::Error::FileSystem(Error::InvalidPath))?;
    let handle = GpioHandle::new(controller, line);

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::DEVICES)
            .await,
    )?;
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::GPIO_DEVICES)
            .await,
    )?;

    crate::mount_peripheral(virtual_file_system, task, path, handle).await
}
