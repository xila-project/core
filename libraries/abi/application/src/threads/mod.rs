//! Cooperative threads and synchronization in native processes.

use core::ffi::{c_char, c_void};

/// A native worker entry. Null is rejected.
pub type ThreadEntry = Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>;

pub const XILA_THREAD_STACK_MIN: usize = 2048;
pub const XILA_THREAD_STACK_MAX: usize = 64 * 1024;
pub const XILA_THREAD_KEYS_MAX: usize = 8;
pub const XILA_MUTEX_RECURSIVE: u32 = 1;
pub const XILA_MUTEX_TRY: u32 = 1;

/// SDK-reserved coroutine-local libc state. Other applications should use TLS keys.
#[repr(C)]
pub struct ThreadLibc {
    error: i32,
    strtok_next: *mut c_char,
    random_state: u64,
}

/// Process-local mutex. Zero initialize for a plain mutex, or use mutex_init.
/// Keep its address fixed from first use through destruction. The fields are private
/// to the host; copying a used mutex or sharing it across processes is invalid.
#[repr(C)]
pub struct Mutex {
    owner: usize,
    depth: u32,
    recursive: bool,
    waiters: *mut c_void,
}

/// Process-local condition variable. Zero initialize or use condition_init.
/// Keep its address fixed from first use through destruction. The fields are private
/// to the host; copying a used condition or sharing it across processes is invalid.
#[repr(C)]
pub struct Condition {
    waiters: *mut c_void,
}

#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
))]
mod runtime;
#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
))]
pub use runtime::*;

#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
)))]
mod unsupported {
    use super::*;
    use crate::{Process, Status};
    include!(concat!(env!("OUT_DIR"), "/disabled_threads.rs"));

    pub(crate) fn bind_process(_: *const Process) {}
    pub(crate) fn stop_workers() {}
    pub(crate) fn exit_process(_: *const Process, _: i32) -> Result<(), Status> {
        Ok(())
    }
}
#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32"
        ),
        target_os = "none"
    )
)))]
pub use unsupported::*;
