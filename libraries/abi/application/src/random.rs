use crate::Status;
use core::{ffi::c_void, slice};
use file_system::Kind;
use synchronization::once_lock::OnceLock;
use task::Manager;
use virtual_file_system::{File, VirtualFileSystem};

const ENTROPY_PATH: &str = "/devices/random/source";
static ENTROPY: OnceLock<&'static VirtualFileSystem> = OnceLock::new();

/// Install the runtime's entropy source once if the system VFS has a character
/// device at `/devices/random/source`. The native executable handler calls this
/// before launch. If the device is absent, a later launch can retry.
pub async fn initialize_random(vfs: &'static VirtualFileSystem) {
    if ENTROPY.try_get().is_none()
        && vfs
            .get_statistics_for_task(Manager::ROOT_TASK_IDENTIFIER, &ENTROPY_PATH)
            .await
            .is_ok_and(|statistics| statistics.kind == Kind::CharacterDevice)
    {
        ENTROPY.get_or_init(|| vfs);
    }
}

/// Fill the caller's buffer with random bytes from the host-granted entropy source.
/// Zero size succeeds without touching the buffer or the source, and permits a
/// null buffer. A pending source suspends the caller's coroutine. On failure the
/// buffer contents are unspecified and must not be used.
/// # Safety
/// Buffer must be writable for size bytes.
pub unsafe extern "C" fn xila_random_fill(buffer: *mut c_void, size: usize) -> Status {
    if size > isize::MAX as usize || (size != 0 && buffer.is_null()) {
        return Status::InvalidArgument;
    }
    if size == 0 {
        return Status::Ok;
    }
    let Some(vfs) = ENTROPY.try_get() else {
        return Status::Unavailable;
    };
    // SAFETY: non-null and at most isize::MAX bytes; the caller guarantees validity.
    let buffer = unsafe { slice::from_raw_parts_mut(buffer.cast(), size) };
    // A native coroutine opens the source with its own VFS permissions.
    // Direct calls by the Rust host use the system task.
    let in_coroutine = crate::is_in_coroutine();
    // The source file is open only inside this future, so a cancelled process drops the
    // future in place and closes it, which keeps the device from reading as busy.
    let result = crate::suspend_on_cancellable(async {
        let task = if in_coroutine {
            vfs.task_manager()
                .try_get_current_task_identifier()
                .await
                .map_err(file_system::IntoIoErrorKind::into_io_error_kind)?
        } else {
            Manager::ROOT_TASK_IDENTIFIER
        };
        File::read_slice_from_character_device(vfs, task, ENTROPY_PATH, buffer).await
    });
    match result {
        Ok(()) => Status::Ok,
        Err(error) => error.into(),
    }
}
