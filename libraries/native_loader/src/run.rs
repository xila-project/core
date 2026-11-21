extern crate alloc;

use alloc::{alloc::alloc, boxed::Box, string::String, vec::Vec};
use core::{alloc::Layout, cell::UnsafeCell, ptr::NonNull};

use abi_application::{File, Heap, HostState, ProcessLaunch, XilaStartup, drive_process};
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

/// The image, process heap and host state of one running program, in a single
/// allocation. The host state holds the startup block, the process capability, the
/// resources they point into and a registry of the handles the program opened, so the
/// coroutine's own frames hold none of it. The process future owns the allocation
/// through [`Owned`], not the coroutine stack.
///
/// A program that is cancelled while suspended has its frames discarded, with no
/// destructor run on them (see `stackful::Coroutine::new`). That is sound because the
/// frames own nothing: [`Owned`] releases the state's resources and handles from the host
/// side, then frees the image, the heap and the state. Freeing the image and the heap is
/// sound for the same reason as before: no driver keeps a pointer into a program buffer
/// between polls, because `BaseOperations` are poll-based, so a suspended coroutine holds
/// no reference the executor or a device could still use.
struct Process {
    _image: LoadedImage,
    heap: UnsafeCell<Heap>,
    state: UnsafeCell<HostState>,
}

/// The owner of a leaked [`Process`] allocation, freeing it on drop.
///
/// It must outlive the program's coroutine, whose frames point into the allocation:
/// [`drive`] holds it beside the coroutine and drops it afterwards, on every path.
struct Owned(NonNull<Process>);

impl Owned {
    /// Move `process` into a new allocation, reporting allocator exhaustion
    /// instead of aborting. The image, the arguments and the streams are released
    /// if this fails.
    fn new(process: Process) -> Result<Self, Error> {
        let layout = Layout::new::<Process>();
        // SAFETY: `Process` holds an image and a heap, so the layout is not zero-sized.
        let Some(pointer) = NonNull::new(unsafe { alloc(layout) }.cast::<Process>()) else {
            // The program can never start, and dropping `process` releases everything it owns.
            return Err(Error::AllocationFailed);
        };
        // SAFETY: the allocation is valid and uninitialized, for the layout of `Process`.
        unsafe { pointer.write(process) };
        Ok(Self(pointer))
    }

    /// The heap of the process. It stays valid until `self` is dropped.
    fn heap(&self) -> NonNull<Heap> {
        // SAFETY: the pointer refers to an initialized `Process`.
        unsafe { NonNull::new_unchecked((*self.0.as_ptr()).heap.get()) }
    }

    /// The host state of the process. It stays at this address until `self` is dropped.
    fn state(&self) -> NonNull<HostState> {
        // SAFETY: the pointer refers to an initialized `Process`.
        unsafe { NonNull::new_unchecked((*self.0.as_ptr()).state.get()) }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        let process = self.0.as_ptr();
        // SAFETY: the pointer came from `Owned::new`, with the global allocator and the
        // layout of `Process`, and the program can no longer run: it never started, it
        // returned or exited, or its coroutine was dropped and its frames discarded. What
        // it still holds is in the state, which is released first: nothing after a normal
        // end, the streams of a program that never started, and the streams, the handles and
        // the graphics objects of one that was cancelled.
        unsafe {
            HostState::release(NonNull::new_unchecked((*process).state.get()));
            drop(Box::from_raw(process));
        }
    }
}

/// Drive the program's coroutine to its end, then free what it ran on. The coroutine is
/// dropped before `owner`, on every path, including the future being dropped while the
/// program is suspended: the temporary that holds it is the newest object of this
/// function, so it is the first to be dropped.
async fn drive(coroutine: Coroutine, owner: Owned) -> i32 {
    let status = drive_process(coroutine).await;
    drop(owner);
    status
}

impl LoadedImage {
    /// Build the coroutine entry of the program and the owner of what it runs on.
    ///
    /// # Safety
    /// Same as [`LoadedImage::spawn`].
    unsafe fn prepare(
        self,
        startup: Startup,
    ) -> Result<(impl FnOnce() -> i32 + 'static, Owned), Error> {
        let entry = self.entry_address();
        let Startup {
            arguments,
            standard_input,
            standard_output,
            standard_error,
            process: launch,
        } = startup;
        let owner = Owned::new(Process {
            _image: self,
            heap: UnsafeCell::new(Heap::new()),
            state: UnsafeCell::new(HostState::new(
                arguments,
                standard_input,
                standard_output,
                standard_error,
            )),
        })?;
        let heap = owner.heap();
        let state = owner.state();
        Ok((move || unsafe { run(entry, heap, state, launch) }, owner))
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
                move |coroutine| drive(coroutine, owner),
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
        Ok(drive(coroutine, owner))
    }
}

/// The coroutine entry. It holds only pointers into the process allocation and the
/// launch request, which the callee consumes before C runs: the arguments, the
/// streams and the startup block stay in the host state, off this stack, so a
/// process that exits early abandons this frame without leaking: see
/// `xila_process_exit`.
unsafe fn run(
    entry: NonNull<u8>,
    heap: NonNull<Heap>,
    state: NonNull<HostState>,
    launch: Option<ProcessLaunch>,
) -> i32 {
    let entry: unsafe extern "C" fn(*const XilaStartup) -> i32 =
        unsafe { core::mem::transmute(entry.as_ptr()) };
    match launch {
        Some(launch) => unsafe { launch.run(state, entry, heap) },
        None => unsafe { HostState::run_linked(state, entry, heap) },
    }
}
