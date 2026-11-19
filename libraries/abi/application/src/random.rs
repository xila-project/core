use crate::{Status, suspend_on};
use core::{ffi::c_void, future::poll_fn, slice};
use file_system::{DirectCharacterDevice, Operation};
use synchronization::once_lock::OnceLock;

static ENTROPY: OnceLock<&'static dyn DirectCharacterDevice> = OnceLock::new();

/// Install the runtime's entropy source once, for example the device mounted at
/// `/devices/random/source`. Later calls are ignored. Until it is installed, a
/// nonempty fill returns Unavailable.
pub fn initialize_random(device: &'static dyn DirectCharacterDevice) {
    ENTROPY.get_or_init(|| device);
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
    let Some(device) = ENTROPY.try_get() else {
        return Status::Unavailable;
    };
    // SAFETY: non-null and at most isize::MAX bytes; the caller guarantees validity.
    let mut remaining: &mut [u8] = unsafe { slice::from_raw_parts_mut(buffer.cast(), size) };
    while !remaining.is_empty() {
        let result = suspend_on(poll_fn(|context| device.read(context, remaining, 0)));
        device.cancel_operation(Operation::Read);
        match result {
            // A source that makes no progress would otherwise loop forever.
            Ok(0) => return Status::InputOutput,
            Ok(count) => {
                let length = remaining.len();
                remaining = &mut core::mem::take(&mut remaining)[count.min(length)..];
            }
            Err(error) => return error.into(),
        }
    }
    Status::Ok
}
