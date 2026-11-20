//! Error vocabulary shared by file systems, the VFS and network transport.
//!
//! Every fallible I/O operation reports an [`IoErrorKind`]: one fieldless,
//! two-byte kind with a stable ID. Errors owned by other subsystems enter the
//! I/O path through [`IntoIoErrorKind`], an explicit mapping at the boundary
//! that never leaks the subsystem's internal step names into user messages.

pub use io_error::{self, Kind as IoErrorKind};

/// Result type of every file system, VFS and device operation.
pub type Result<T> = core::result::Result<T, IoErrorKind>;

/// Explicit translation of a subsystem error into an I/O kind.
///
/// Implemented by the crate that owns the source error, so the shared
/// `io_error` crate stays free of subsystem dependencies.
pub trait IntoIoErrorKind {
    /// Return the I/O kind that best describes this failure for a caller.
    fn into_io_error_kind(self) -> IoErrorKind;
}

impl IntoIoErrorKind for IoErrorKind {
    fn into_io_error_kind(self) -> IoErrorKind {
        self
    }
}

impl IntoIoErrorKind for task::Error {
    fn into_io_error_kind(self) -> IoErrorKind {
        use task::Error::*;
        match self {
            InvalidTaskIdentifier | ThreadNotRegistered | NoThreadForTask => {
                IoErrorKind::NoSuchTask
            }
            InvalidSpawnerIdentifier | InvalidEnvironmentVariable => IoErrorKind::InvalidArgument,
            ThreadAlreadyRegistered | AlreadyInitialized | AlreadySet => IoErrorKind::InvalidState,
            FailedToCreateThread | FailedToSpawnThread | TooManyTasks | TooManySpawners => {
                IoErrorKind::OutOfMemory
            }
            NotInitialized | NoSpawnerAvailable => IoErrorKind::DeviceUnavailable,
        }
    }
}

impl IntoIoErrorKind for users::Error {
    fn into_io_error_kind(self) -> IoErrorKind {
        use users::Error::*;
        match self {
            DuplicateGroupIdentifier
            | DuplicateGroupName
            | DuplicateUserIdentifier
            | DuplicateUserName => IoErrorKind::AlreadyExists,
            // A stale owner or group is a broken invariant, not a caller mistake.
            InvalidGroupIdentifier | InvalidUserIdentifier | PoisonedLock => {
                IoErrorKind::InputOutput
            }
            TooManyGroups | TooManyUsers => IoErrorKind::NoSpace,
            NotInitialized => IoErrorKind::DeviceUnavailable,
            AlreadyInitialized => IoErrorKind::InvalidState,
        }
    }
}

/// Convert a result whose error implements [`IntoIoErrorKind`].
pub trait ResultExt<T> {
    /// Translate the error into an [`IoErrorKind`].
    fn map_io_error(self) -> Result<T>;
}

impl<T, E: IntoIoErrorKind> ResultExt<T> for core::result::Result<T, E> {
    fn map_io_error(self) -> Result<T> {
        self.map_err(IntoIoErrorKind::into_io_error_kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_io_async::{Error as _, ErrorKind};

    #[test]
    fn a_kind_keeps_its_meaning_through_embedded_io() {
        for (kind, expected) in [
            (IoErrorKind::NotFound, ErrorKind::NotFound),
            (IoErrorKind::PermissionDenied, ErrorKind::PermissionDenied),
            (IoErrorKind::AlreadyExists, ErrorKind::AlreadyExists),
            (IoErrorKind::OutOfMemory, ErrorKind::OutOfMemory),
            (IoErrorKind::Unsupported, ErrorKind::Unsupported),
            // Generic I/O must never ask a caller to retry with `Interrupted`.
            (IoErrorKind::InputOutput, ErrorKind::Other),
            // Storage exhaustion is not memory exhaustion.
            (IoErrorKind::NoSpace, ErrorKind::Other),
        ] {
            assert_eq!(kind.kind(), expected, "{kind:?}");
        }
    }

    #[test]
    fn task_errors_keep_their_cause() {
        use task::Error::*;
        for (error, kind) in [
            (InvalidTaskIdentifier, IoErrorKind::NoSuchTask),
            (NoThreadForTask, IoErrorKind::NoSuchTask),
            (InvalidSpawnerIdentifier, IoErrorKind::InvalidArgument),
            (AlreadySet, IoErrorKind::InvalidState),
            (TooManyTasks, IoErrorKind::OutOfMemory),
            (NoSpawnerAvailable, IoErrorKind::DeviceUnavailable),
        ] {
            assert_eq!(error.into_io_error_kind(), kind);
        }
    }

    #[test]
    fn users_errors_keep_their_cause() {
        use users::Error::*;
        for (error, kind) in [
            (DuplicateUserName, IoErrorKind::AlreadyExists),
            (InvalidUserIdentifier, IoErrorKind::InputOutput),
            (PoisonedLock, IoErrorKind::InputOutput),
            (TooManyUsers, IoErrorKind::NoSpace),
            (NotInitialized, IoErrorKind::DeviceUnavailable),
            (AlreadyInitialized, IoErrorKind::InvalidState),
        ] {
            assert_eq!(error.into_io_error_kind(), kind);
        }
    }

    #[test]
    fn results_convert_at_the_call_site() {
        let failed: core::result::Result<(), task::Error> = Err(task::Error::InvalidTaskIdentifier);
        assert_eq!(failed.map_io_error(), Err(IoErrorKind::NoSuchTask));
        let passed: core::result::Result<u8, users::Error> = Ok(3);
        assert_eq!(passed.map_io_error(), Ok(3));
    }
}
