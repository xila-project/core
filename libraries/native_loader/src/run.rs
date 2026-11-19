extern crate alloc;

use alloc::{boxed::Box, string::String, vec::Vec};
use core::ptr::NonNull;

use abi_application::{Directory, File, Heap, XilaStartup, run_main};

use crate::LoadedImage;

/// Capabilities and arguments owned by the loaded program's coroutine.
#[derive(Default)]
pub struct Startup {
    pub arguments: Vec<String>,
    pub standard_input: Option<File>,
    pub standard_output: Option<File>,
    pub standard_error: Option<File>,
    pub home: Option<Directory>,
}

/// The image and process heap of one running program. The task future owns this
/// value, not the coroutine stack, so cancelling a suspended coroutine (which
/// leaks its stack) still releases both once the task future is dropped.
struct Process {
    _image: LoadedImage,
    heap: NonNull<Heap>,
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: `heap` came from `Box::into_raw` and the program can no longer
        // run: it returned, or its suspended coroutine was cancelled and is never resumed.
        drop(unsafe { Box::from_raw(self.heap.as_ptr()) });
    }
}

impl LoadedImage {
    /// Run the ELF entry point on the calling task's executor.
    ///
    /// The image and the process heap are released when the task finishes or is
    /// cancelled. A cancelled coroutine's stack, argument strings and startup
    /// streams stay leaked, as stackful documents.
    ///
    /// # Safety
    /// The image must come from a trusted source and implement
    /// `int32_t _xila_start(const XilaStartup *)`. Its entry and callees must
    /// obey the startup lifetime rules, fit on the stack, and return without
    /// unwinding or leaving code or data references in other tasks. The caller
    /// must meet the executor affinity and single-core requirements of
    /// `stackful::spawn`.
    pub async unsafe fn spawn<T: 'static>(
        self,
        manager: &'static task::Manager,
        parent: task::TaskIdentifier,
        name: &str,
        stack: stackful::Stack,
        startup: Startup,
        map: impl FnOnce(i32) -> T + 'static,
    ) -> task::Result<task::JoinHandle<T>> {
        let entry = self.entry_address();
        let heap = NonNull::from(Box::leak(Box::new(Heap::new())));
        let process = Process { _image: self, heap };
        unsafe {
            stackful::spawn_with_result(
                manager,
                parent,
                name,
                stack,
                move || run(entry, heap, startup),
                move |status| {
                    let result = map(status);
                    drop(process);
                    result
                },
            )
            .await
            .map(|(handle, _)| handle)
        }
    }
}

unsafe fn run(entry: NonNull<u8>, mut heap: NonNull<Heap>, startup: Startup) -> i32 {
    let entry: unsafe extern "C" fn(*const XilaStartup) -> i32 =
        unsafe { core::mem::transmute(entry.as_ptr()) };
    let args: Vec<&str> = startup.arguments.iter().map(String::as_str).collect();
    unsafe {
        run_main(
            entry,
            heap.as_mut(),
            &args,
            startup.standard_input,
            startup.standard_output,
            startup.standard_error,
            startup.home,
        )
    }
}
