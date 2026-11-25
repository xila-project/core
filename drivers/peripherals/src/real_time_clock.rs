//! Real-time clock device node `/devices/time/real_time_clock`.
//!
//! [`RealTimeClockHandle`] adapts a `peripherals::real_time_clock::RealTimeClockController`
//! lower half to [`PeripheralHandle`]. Plain `read` and `write` on the node fail
//! with `Unsupported`: the clock is only reachable through the control commands
//! of the `device::real_time_clock` contract. Like Linux, the RTC stores
//! wall-clock time (UTC) only; timers and deadlines stay on the monotonic
//! clock of the `time` subsystem.
//!
//! # Control commands (kind `b'r'`)
//!
//! | Number | Command    | Direction | Input                  | Output                 | Effect                                              |
//! |--------|------------|-----------|------------------------|------------------------|-----------------------------------------------------|
//! | `0x00` | `GET_TIME` | Read      | `RealTimeClockTimeAbi` | `RealTimeClockTimeAbi` | Returns the time the clock holds and its validity.  |
//! | `0x01` | `SET_TIME` | Write     | `RealTimeClockTimeAbi` | `RealTimeClockTimeAbi` | Sets the clock, then returns the time it now holds. |
//!
//! A wrong `version`, a non-zero `reserved` or, for `SET_TIME`, `valid != 1` or
//! `nanoseconds >= 1_000_000_000` is `InvalidArgument`. A controller failure is
//! `InputOutput`, except `Busy` which is `ResourceBusy` and `Invalid` which is
//! `InvalidArgument`.

#[cfg(feature = "external-real-time-clock")]
mod external;

#[cfg(feature = "external-real-time-clock")]
pub use external::ExternalRealTimeClock;

use file_system::{ControlCommand, ControlCommandIdentifier, IoErrorKind, Result};
use peripherals::real_time_clock::{Error as ControllerError, RealTimeClockController, Time};
use shared::AnyByLayout;

use crate::PeripheralHandle;

pub use device::real_time_clock::{
    ABI_VERSION, GET_TIME, MAX_NANOSECONDS, NOT_VALID, REAL_TIME_CLOCK_PATH, RealTimeClockTimeAbi,
    SET_TIME, VALID,
};

/// One real-time clock controller adapted to [`PeripheralHandle`].
///
/// The controller is shared (`&'static` in practice): the same lower half also
/// serves the time subsystem's start-up read and write-back task, which must
/// not go through this exclusive device node.
pub struct RealTimeClockHandle<'a, C: RealTimeClockController + ?Sized> {
    controller: &'a C,
}

impl<'a, C: RealTimeClockController + ?Sized> RealTimeClockHandle<'a, C> {
    pub const fn new(controller: &'a C) -> Self {
        Self { controller }
    }

    /// Encodes a controller reading as the contract payload.
    fn payload(time: Option<Time>) -> RealTimeClockTimeAbi {
        match time {
            Some(time) => RealTimeClockTimeAbi {
                version: ABI_VERSION,
                valid: VALID,
                reserved: 0,
                nanoseconds: time.nanoseconds,
                seconds: time.seconds,
            },
            None => RealTimeClockTimeAbi {
                version: ABI_VERSION,
                valid: NOT_VALID,
                reserved: 0,
                nanoseconds: 0,
                seconds: 0,
            },
        }
    }
}

impl<C> PeripheralHandle for RealTimeClockHandle<'_, C>
where
    C: RealTimeClockController + ?Sized,
{
    /// The node is control-only: plain reads fail with `Unsupported`.
    fn read(&mut self, _buffer: &mut [u8]) -> Result<usize> {
        Err(IoErrorKind::Unsupported)
    }

    /// The node is control-only: plain writes fail with `Unsupported`.
    fn write(&mut self, _buffer: &[u8]) -> Result<usize> {
        Err(IoErrorKind::Unsupported)
    }

    fn control(
        &mut self,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Result<()> {
        if command == GET_TIME::IDENTIFIER {
            let request = GET_TIME::cast_input(input)?;
            check_header(request.version, request.reserved)?;
            let time = self.controller.read().map_err(map_error)?;
            *GET_TIME::cast_output(output)? = Self::payload(time);
        } else if command == SET_TIME::IDENTIFIER {
            let request = SET_TIME::cast_input(input)?;
            if request.version != ABI_VERSION
                || request.reserved != 0
                || request.valid != VALID
                || request.nanoseconds >= MAX_NANOSECONDS
            {
                return Err(IoErrorKind::InvalidArgument);
            }
            let time = Time::new(request.seconds, request.nanoseconds);
            self.controller.write(time).map_err(map_error)?;
            let read_back = self.controller.read().map_err(map_error)?;
            *SET_TIME::cast_output(output)? = Self::payload(read_back);
        } else {
            return Err(IoErrorKind::Unsupported);
        }
        Ok(())
    }
}

fn check_header(version: u16, reserved: u8) -> Result<()> {
    if version == ABI_VERSION && reserved == 0 {
        Ok(())
    } else {
        Err(IoErrorKind::InvalidArgument)
    }
}

/// Maps a controller error to the file system error seen by applications.
fn map_error(error: ControllerError) -> IoErrorKind {
    match error {
        ControllerError::Invalid => IoErrorKind::InvalidArgument,
        ControllerError::Busy => IoErrorKind::ResourceBusy,
        ControllerError::Other => IoErrorKind::InputOutput,
    }
}

/// Mounts `controller` at `/devices/time/real_time_clock`, creating `/devices`
/// and `/devices/time` when they do not exist.
///
/// The node is mounted with [`file_system::Permissions::PRIVILEGED_DEVICE`]:
/// the mounting task (root at boot) may write, everyone else may only read, so
/// `SET_TIME` and a write-mode open are reserved to root while `GET_TIME` stays
/// readable by all users.
#[cfg(feature = "vfs")]
pub async fn mount_real_time_clock<C>(
    virtual_file_system: &virtual_file_system::VirtualFileSystem,
    task: task::TaskIdentifier,
    controller: &'static C,
) -> virtual_file_system::Result<()>
where
    C: RealTimeClockController + ?Sized + 'static,
{
    use file_system::Path;

    let handle = RealTimeClockHandle::new(controller);

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::DEVICES)
            .await,
    )?;
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &Path::TIME_DEVICES)
            .await,
    )?;

    crate::mount_peripheral_with_permissions(
        virtual_file_system,
        task,
        Path::REAL_TIME_CLOCK,
        handle,
        file_system::Permissions::PRIVILEGED_DEVICE,
    )
    .await
}
