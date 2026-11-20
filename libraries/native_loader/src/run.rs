extern crate alloc;

use alloc::{alloc::alloc, boxed::Box, string::String, vec::Vec};
use core::{alloc::Layout, cell::UnsafeCell, ptr::NonNull};

use abi_application::{File, Heap, ProcessLaunch, XilaStartup, drive_process, run_main};

use crate::{Error, LoadedImage};

/// Capabilities and arguments owned by the loaded program's coroutine.
#[derive(Default)]
pub struct Startup {
    pub arguments: Vec<String>,
    pub standard_input: Option<File>,
    pub standard_output: Option<File>,
    pub standard_error: Option<File>,
    /// Makes the program a process: it gets a process capability, its explicit
    /// environment if any and its home directory. Without it the program is
    /// started like a linked one, with none of these, and cannot exit early.
    pub process: Option<ProcessLaunch>,
}

/// The image and process heap of one running program, in a single allocation.
/// The task future owns it through [`Owned`], not the coroutine stack, so
/// cancelling a suspended coroutine (which leaks its stack) still releases both
/// once the task future is dropped.
///
/// Freeing the image and the heap while the cancelled coroutine's own pending
/// future is leaked is sound because no driver keeps a pointer into a program
/// buffer between polls: `BaseOperations` are poll-based, so a suspended
/// coroutine holds no reference the executor or a device could still use.
struct Process {
    _image: LoadedImage,
    heap: UnsafeCell<Heap>,
}

/// The owner of a leaked [`Process`] allocation, freeing it on drop.
struct Owned(NonNull<Process>);

impl Owned {
    /// Move `process` into a new allocation, reporting allocator exhaustion
    /// instead of aborting. The image is released if this fails.
    fn new(process: Process) -> Result<Self, Error> {
        let layout = Layout::new::<Process>();
        // SAFETY: `Process` holds an image and a heap, so the layout is not zero-sized.
        let pointer = NonNull::new(unsafe { alloc(layout) }.cast::<Process>())
            .ok_or(Error::AllocationFailed)?;
        // SAFETY: the allocation is valid and uninitialized, for the layout of `Process`.
        unsafe { pointer.write(process) };
        Ok(Self(pointer))
    }

    /// The heap of the process. It stays valid until `self` is dropped.
    fn heap(&self) -> NonNull<Heap> {
        // SAFETY: the pointer refers to an initialized `Process`.
        unsafe { NonNull::new_unchecked((*self.0.as_ptr()).heap.get()) }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `Owned::new`, with the global allocator and
        // the layout of `Process`, and the program can no longer run: it returned,
        // or its suspended coroutine was cancelled and is never resumed.
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

impl LoadedImage {
    /// Prepare to run the ELF entry point on the calling task's executor.
    ///
    /// Allocating the process state can fail with [`Error::AllocationFailed`],
    /// which releases the image. Otherwise the returned future spawns the task
    /// and resolves to its join handle and identifier, like
    /// `stackful::spawn_with_result`.
    ///
    /// The image and the process heap are released when the task finishes or is
    /// cancelled, or when the future is dropped before it completes. A
    /// cancelled coroutine's stack, argument strings and startup streams stay
    /// leaked, as stackful documents.
    ///
    /// # Safety
    /// The image must come from a trusted source and implement
    /// `int32_t _xila_start(const XilaStartup *)`. Its entry and callees must
    /// obey the startup lifetime rules, fit on the stack, and return without
    /// unwinding or leaving code or data references in other tasks. The caller
    /// must meet the executor affinity and single-core requirements of
    /// `stackful::spawn`.
    pub unsafe fn spawn<T: 'static>(
        self,
        manager: &'static task::Manager,
        parent: task::TaskIdentifier,
        name: &str,
        stack: stackful::Stack,
        startup: Startup,
        map: impl FnOnce(i32) -> T + 'static,
    ) -> Result<
        impl Future<Output = task::Result<(task::JoinHandle<T>, task::TaskIdentifier)>>,
        Error,
    > {
        let entry = self.entry_address();
        let process = Owned::new(Process {
            _image: self,
            heap: UnsafeCell::new(Heap::new()),
        })?;
        let heap = process.heap();
        let managed = startup.process.is_some();
        Ok(unsafe {
            stackful::spawn_with_driver(
                manager,
                parent,
                name,
                stack,
                move || run(entry, heap, startup),
                move |coroutine| async move {
                    if managed {
                        drive_process(coroutine).await
                    } else {
                        stackful::drive(coroutine).await
                    }
                },
                move |status| {
                    let result = map(status);
                    drop(process);
                    result
                },
            )
        })
    }
}

/// Everything owned moves into the callee, so a process that exits early abandons
/// this frame without leaking: see `xila_process_exit`.
unsafe fn run(entry: NonNull<u8>, mut heap: NonNull<Heap>, startup: Startup) -> i32 {
    let entry: unsafe extern "C" fn(*const XilaStartup) -> i32 =
        unsafe { core::mem::transmute(entry.as_ptr()) };
    let Startup {
        arguments,
        standard_input,
        standard_output,
        standard_error,
        process,
    } = startup;
    if let Some(process) = process {
        return unsafe {
            process.run(
                entry,
                heap.as_mut(),
                arguments,
                standard_input,
                standard_output,
                standard_error,
            )
        };
    }
    let args: Vec<&str> = arguments.iter().map(String::as_str).collect();
    unsafe {
        run_main(
            entry,
            heap.as_mut(),
            &args,
            standard_input,
            standard_output,
            standard_error,
            None,
        )
    }
}
