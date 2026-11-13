//! System power controller contract.
//!
//! A target implements only the actions it really performs; the defaults
//! report [`Error::Unsupported`] instead of pretending. Each method is called
//! after the OS lifecycle stopped tasks and committed storage, and may never
//! return (process exit, hardware reset). It runs on the executor: it must not
//! wait for other tasks.

/// Error categories reported by a power controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The target does not provide this action.
    Unsupported,
    /// Any other hardware or driver failure.
    Other,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "unsupported power action",
            Self::Other => "power action failed",
        })
    }
}

impl core::error::Error for Error {}

/// Hardware-facing power actions.
pub trait PowerController {
    /// Actions that succeed on this target, as `device::power::Capabilities`
    /// bits: `1` power off, `2` restart, `4` sleep.
    fn capabilities(&self) -> u8;

    /// Halt or remove power. `status` is a process exit status on hosted
    /// targets and ignored elsewhere.
    fn power_off(&mut self, status: u8) -> Result<(), Error> {
        let _ = status;
        Err(Error::Unsupported)
    }

    /// Reset the system or restart the runtime instance.
    fn restart(&mut self, status: u8) -> Result<(), Error> {
        let _ = status;
        Err(Error::Unsupported)
    }

    /// Enter a low-power state the system resumes from.
    fn sleep(&mut self) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
}
