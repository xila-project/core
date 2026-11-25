//! Privileged manual UTC setting (feature `services`).
//!
//! Shell commands like `date --set` set UTC by hand. Only the root user may do
//! it: the check runs here, in the runtime-owned API, so a guest or a
//! non-privileged executable cannot change the clock through it. A successful
//! set uses [`ClockSource::Manual`] and, through the manager's change
//! notification, triggers the real-time clock write-back.

use core::time::Duration;

use task::TaskIdentifier;

use crate::{ClockSource, Manager, RealtimeSample, Timestamp};

/// Error of [`set_realtime_manual`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualError {
    /// The calling task does not run as root, or its user cannot be resolved.
    PermissionDenied,
    /// The manager rejected the sample.
    Clock(crate::Error),
}

impl core::fmt::Display for ManualError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PermissionDenied => formatter.write_str("setting UTC manually requires root"),
            Self::Clock(error) => write!(formatter, "time manager rejected the sample: {error:?}"),
        }
    }
}

/// Sets UTC manually from `timestamp`, as root only.
///
/// `task` identifies the calling task; its user must be root. The sample is
/// recorded with [`ClockSource::Manual`], so the real-time clock write-back
/// task stores it in the RTC. A shell-level whole-second input is reported with
/// a one-second resolution; a subsecond input with a nanosecond resolution.
pub async fn set_realtime_manual(
    task_manager: &task::Manager,
    task: TaskIdentifier,
    manager: &Manager<'_>,
    timestamp: Timestamp,
) -> Result<(), ManualError> {
    let user = task_manager
        .get_user(task)
        .await
        .map_err(|_| ManualError::PermissionDenied)?;
    if !users::Manager::is_root(user) {
        return Err(ManualError::PermissionDenied);
    }
    let resolution = if timestamp.as_nanos() % 1_000_000_000 == 0 {
        Duration::from_secs(1)
    } else {
        Duration::from_nanos(1)
    };
    manager
        .set_realtime(RealtimeSample {
            timestamp,
            source: ClockSource::Manual,
            resolution,
            accuracy: None,
        })
        .map_err(ManualError::Clock)?;
    Ok(())
}
