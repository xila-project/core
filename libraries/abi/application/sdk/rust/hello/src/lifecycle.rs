use super::write_output;
use core::{
    ffi::c_void,
    ptr, slice,
    sync::atomic::{AtomicU8, Ordering},
};
use xila_sys::{Status, XilaStartup, runtime};

pub(super) static INITIALIZED: AtomicU8 = AtomicU8::new(0);

#[used]
#[unsafe(link_section = ".preinit_array")]
static PREINITIALIZER: extern "C" fn() = preinitialize;
#[used]
#[unsafe(link_section = ".init_array")]
static INITIALIZER: extern "C" fn() = initialize;
#[used]
#[unsafe(link_section = ".fini_array")]
static FINALIZER: extern "C" fn() = finalize;

extern "C" fn preinitialize() {
    if write_output(b"Rust preinit\n") {
        INITIALIZED.store(1, Ordering::Relaxed);
    }
}

extern "C" fn initialize() {
    if INITIALIZED.load(Ordering::Relaxed) == 1 && write_output(b"Rust init\n") {
        INITIALIZED.store(2, Ordering::Relaxed);
    }
}

extern "C" fn finalize() {
    write_output(b"Rust fini\n");
}

pub(super) unsafe fn worker_requested(startup: *const XilaStartup) -> bool {
    let startup = unsafe { &*startup };
    if startup.argument_count != 1 {
        return false;
    }
    let argument = unsafe { &*startup.arguments };
    const WORKERS: &[u8] = b"workers";
    argument.length == WORKERS.len()
        && unsafe { slice::from_raw_parts(argument.data.cast::<u8>(), argument.length) } == WORKERS
}

pub(super) fn start_worker() -> bool {
    let (Some(api), Some(startup)) = (runtime::api(), runtime::startup()) else {
        return false;
    };
    let mut thread = 0;
    let mut live = false;
    unsafe {
        (api.thread_spawn)(
            startup.process,
            0,
            Some(worker),
            ptr::null_mut(),
            &mut thread,
        ) == Status::Ok
            && (api.thread_has_workers)(&mut live) == Status::Ok
            && live
    }
}

unsafe extern "C" fn worker(argument: *mut c_void) -> *mut c_void {
    argument
}
