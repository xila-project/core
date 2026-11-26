extern crate alloc;

use alloc::{string::String, vec::Vec};

use abi_application::{File, NativeEntry, ProcessLaunch, ProcessOwner, drive_owned};
use stackful::Coroutine;

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

impl LoadedImage {
    /// Build the coroutine entry of the program and the owner of what it runs on.
    ///
    /// # Safety
    /// Same as [`LoadedImage::spawn`].
    unsafe fn prepare(
        self,
        startup: Startup,
    ) -> Result<(impl FnOnce() -> i32 + 'static, ProcessOwner<Self>), Error> {
        // SAFETY: the loader validated the entry point, and the owner keeps the image
        // mapped until the program can no longer run.
        let entry: NativeEntry = unsafe { core::mem::transmute(self.entry_address().as_ptr()) };
        let Startup {
            arguments,
            standard_input,
            standard_output,
            standard_error,
            process: launch,
        } = startup;
        let owner = ProcessOwner::from_startup(
            self,
            arguments,
            standard_input,
            standard_output,
            standard_error,
        )
        .map_err(|_| Error::AllocationFailed)?;
        let run = unsafe { owner.entry(entry, launch) };
        Ok((run, owner))
    }

    /// Prepare to run the ELF entry point on the calling task's executor.
    ///
    /// Allocating the process state can fail with [`Error::AllocationFailed`],
    /// which releases the image, the arguments and the streams. Otherwise the returned
    /// future spawns the task and resolves to its join handle and identifier, like
    /// `stackful::spawn_with_result`.
    ///
    /// The image, the process heap and the host state are released when the task finishes
    /// or is cancelled, or when the future is dropped before it completes. So is
    /// everything the program holds. A program that returns, exits or is cancelled has
    /// its arguments, its startup streams and its graphics objects released, and so are
    /// the files, directories, listings, children and sockets it opened and did not close.
    /// A cancelled program is a task future that is dropped while its coroutine is
    /// suspended: its coroutine stack is freed, with no destructor run on it.
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
        let (entry, owner) = unsafe { self.prepare(startup)? };
        Ok(unsafe {
            stackful::spawn_with_driver(
                manager,
                parent,
                name,
                stack,
                entry,
                move |coroutine| drive_owned(coroutine, owner.erase()),
                map,
            )
        })
    }

    /// Run the ELF entry point on the calling task, as a future that resolves to its exit
    /// status, without spawning a task for it.
    ///
    /// Dropping the future before it completes cancels the program, as dropping the task
    /// future of [`LoadedImage::spawn`] does: its coroutine stack is freed, and everything
    /// it held is released. The program runs inside the task that polls the future, which is
    /// how a process learns its identity.
    ///
    /// # Safety
    /// Same as [`LoadedImage::spawn`]; the future must be polled on the executor that
    /// created it.
    pub unsafe fn start(
        self,
        stack: stackful::Stack,
        startup: Startup,
    ) -> Result<impl Future<Output = i32> + 'static, Error> {
        let (entry, owner) = unsafe { self.prepare(startup)? };
        let coroutine = unsafe { Coroutine::new(stack, entry) };
        Ok(drive_owned(coroutine, owner.erase()))
    }
}
