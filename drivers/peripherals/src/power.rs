//! System power device node (`/devices/power`).
//!
//! [`PowerHandle`] adapts a `peripherals::power::PowerController` to
//! [`PeripheralHandle`]. Plain reads and writes are unsupported; the typed
//! commands of `device::power` are the whole interface. Mount the node so
//! that only privileged users can open it: the commands carry no caller
//! identity, so the VFS permission check at open time is the authorization.
//! The OS lifecycle opens this node as the last step of a shutdown.

use file_system::{ControlCommand, ControlCommandIdentifier, Error, Result};
use peripherals::power::{Error as PowerError, PowerController};
use shared::AnyByLayout;

use crate::PeripheralHandle;

pub use device::power::{Capabilities, GET_CAPABILITIES, POWER_OFF, RESTART, SLEEP};

/// Adapts a power controller to [`PeripheralHandle`].
pub struct PowerHandle<C>(C);

impl<C: PowerController> PowerHandle<C> {
    pub const fn new(controller: C) -> Self {
        Self(controller)
    }
}

impl<C> PeripheralHandle for PowerHandle<C>
where
    C: PowerController + Send,
{
    fn read(&mut self, _buffer: &mut [u8]) -> Result<usize> {
        Err(Error::UnsupportedOperation)
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
        if command == GET_CAPABILITIES::IDENTIFIER {
            *GET_CAPABILITIES::cast_output(output)? = self.0.capabilities();
            Ok(())
        } else if command == POWER_OFF::IDENTIFIER {
            let status = *POWER_OFF::cast_input(input)?;
            self.0.power_off(status).map_err(map_error)
        } else if command == RESTART::IDENTIFIER {
            let status = *RESTART::cast_input(input)?;
            self.0.restart(status).map_err(map_error)
        } else if command == SLEEP::IDENTIFIER {
            self.0.sleep().map_err(map_error)
        } else {
            Err(Error::UnsupportedOperation)
        }
    }
}

/// Maps a power controller error to the file system error seen by callers.
fn map_error(error: PowerError) -> Error {
    match error {
        PowerError::Unsupported => Error::UnsupportedOperation,
        PowerError::Other => Error::InputOutput,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restart-only controller recording the last request.
    #[derive(Default)]
    struct ResetOnly {
        restarted: Option<u8>,
    }

    impl PowerController for ResetOnly {
        fn capabilities(&self) -> u8 {
            Capabilities::RESTART.0
        }

        fn restart(&mut self, status: u8) -> core::result::Result<(), PowerError> {
            self.restarted = Some(status);
            Ok(())
        }
    }

    fn control<C: ControlCommand>(
        handle: &mut PowerHandle<ResetOnly>,
        input: &C::Input,
        output: &mut C::Output,
    ) -> Result<()> {
        handle.control(
            C::IDENTIFIER,
            AnyByLayout::from(input),
            AnyByLayout::from_mutable(output),
        )
    }

    #[test]
    fn typed_commands_reach_the_controller_and_unsupported_actions_fail() {
        let mut handle = PowerHandle::new(ResetOnly::default());
        let mut capabilities = 0;
        control::<GET_CAPABILITIES>(&mut handle, &(), &mut capabilities).unwrap();
        assert_eq!(capabilities, Capabilities::RESTART.0);

        assert_eq!(
            control::<POWER_OFF>(&mut handle, &0, &mut ()),
            Err(Error::UnsupportedOperation)
        );
        assert_eq!(
            control::<SLEEP>(&mut handle, &(), &mut ()),
            Err(Error::UnsupportedOperation)
        );
        control::<RESTART>(&mut handle, &7, &mut ()).unwrap();
        assert_eq!(handle.0.restarted, Some(7));
    }

    #[test]
    fn raw_bytes_cannot_trigger_an_action() {
        let mut handle = PowerHandle::new(ResetOnly::default());
        assert_eq!(handle.write(b"reboot"), Err(Error::UnsupportedOperation));
        assert_eq!(handle.0.restarted, None);
    }
}
