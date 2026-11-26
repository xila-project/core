//! Run a native entry as a process on the calling task's executor.
//!
//! The loader of native images and the adapter of statically linked programs share this
//! runner. Both need the same cancellation-safe ownership: the heap and the host state
//! live in one allocation off the coroutine stack, and a driver releases them on every
//! path (return, `xila_process_exit` and a dropped future).
extern crate alloc;

use crate::{File, Heap, HostState, ProcessLaunch, Status, XilaStartup, drive_process};
use alloc::{alloc::alloc, boxed::Box, string::String, vec::Vec};
use core::{alloc::Layout, cell::UnsafeCell, future::Future, ptr::NonNull};
use stackful::{Coroutine, Stack};

/// The entry of a native program, as the host calls it.
pub type NativeEntry = unsafe extern "C" fn(*const XilaStartup) -> i32;

/// Everything one running program owns on the host side, in a single allocation: a value
/// that must outlive the program's code (the loaded image, for a loader), the process
/// heap and the host state. The host state holds the startup block, the process
/// capability, the resources they point into and a registry of the handles the program
/// opened, so the coroutine's own frames hold none of it.
///
/// A program that is cancelled while suspended has its frames discarded, with no
/// destructor run on them (see `stackful::Coroutine::new`). That is sound because the
/// frames own nothing: dropping [`ProcessOwner`] releases the state's resources and handles
/// from the host side, then frees the heap, the state and, last, the keep-alive value. No
/// driver keeps a pointer into a program buffer between polls, because `BaseOperations` are
/// poll-based, so a suspended coroutine holds no reference that the executor or a device
/// could still use.
struct Record<K> {
    heap: UnsafeCell<Heap>,
    state: UnsafeCell<HostState>,
    /// Declared last, so it is dropped after the heap and the state: a kept image holds the
    /// code of the program, so it stays mapped until nothing can run it any more.
    _keep: K,
}

/// The owner of a leaked `Record` allocation, freeing it on drop.
///
/// It must outlive the program's coroutine, whose frames point into the allocation:
/// [`drive_owned`] holds it beside the coroutine and drops it afterwards, on every path.
pub struct ProcessOwner<K = ()>(NonNull<Record<K>>);

impl<K> ProcessOwner<K> {
    /// Move the state and `keep` into a new allocation, reporting allocator exhaustion
    /// instead of aborting. Both are released if this fails.
    pub fn new(keep: K, state: HostState) -> Result<Self, Status> {
        let layout = Layout::new::<Record<K>>();
        // SAFETY: the heap makes the record larger than zero bytes.
        let Some(pointer) = NonNull::new(unsafe { alloc(layout) }.cast::<Record<K>>()) else {
            // The program can never start, and dropping the arguments releases everything
            // the state and `keep` own.
            return Err(Status::OutOfMemory);
        };
        // SAFETY: the allocation is valid and uninitialized, for the layout of `Record`.
        unsafe {
            pointer.write(Record {
                heap: UnsafeCell::new(Heap::new()),
                state: UnsafeCell::new(state),
                _keep: keep,
            });
        }
        Ok(Self(pointer))
    }

    /// Build the host state from a program's arguments and standard streams, then
    /// [`ProcessOwner::new`].
    pub fn from_startup(
        keep: K,
        arguments: Vec<String>,
        standard_input: Option<File>,
        standard_output: Option<File>,
        standard_error: Option<File>,
    ) -> Result<Self, Status> {
        Self::new(
            keep,
            HostState::new(arguments, standard_input, standard_output, standard_error),
        )
    }

    /// The heap of the process. It stays valid until `self` is dropped.
    pub fn heap(&self) -> NonNull<Heap> {
        // SAFETY: the pointer refers to an initialized record.
        unsafe { NonNull::new_unchecked((*self.0.as_ptr()).heap.get()) }
    }

    /// The host state of the process. It stays at this address until `self` is dropped.
    pub fn state(&self) -> NonNull<HostState> {
        // SAFETY: the pointer refers to an initialized record.
        unsafe { NonNull::new_unchecked((*self.0.as_ptr()).state.get()) }
    }

    /// The coroutine entry of the program. It holds only pointers into the allocation and
    /// the launch request, which the callee consumes before native code runs: the
    /// arguments, the streams and the startup block stay in the host state, off the
    /// coroutine stack, so a process that exits early abandons this frame without leaking
    /// (see `xila_process_exit`).
    ///
    /// Without a `launch` the program is started like a linked one with no process
    /// capability: it gets none of the process services and cannot exit early.
    ///
    /// # Safety
    /// `entry` obeys the startup lifetime rules, fits on the coroutine stack, and returns
    /// without unwinding. It stays callable until the owner is dropped. The returned closure
    /// must be invoked at most once, and only inside a coroutine that is dropped before the
    /// owner (see [`drive_owned`]).
    pub unsafe fn entry(
        &self,
        entry: NativeEntry,
        launch: Option<ProcessLaunch>,
    ) -> impl FnOnce() -> i32 + 'static + use<K> {
        let heap = self.heap();
        let state = self.state();
        move || unsafe {
            match launch {
                Some(launch) => launch.run(state, entry, heap),
                None => HostState::run_linked(state, entry, heap),
            }
        }
    }
}

impl<K> Drop for ProcessOwner<K> {
    fn drop(&mut self) {
        let record = self.0.as_ptr();
        // SAFETY: the pointer came from `ProcessOwner::new`, with the global allocator and
        // the layout of `Record`, and the program can no longer run: it never started, it
        // returned or exited, or its coroutine was dropped and its frames discarded. What
        // it still holds is in the state, which is released first: nothing after a normal
        // end, the streams of a program that never started, and the streams, the handles
        // and the graphics objects of one that was cancelled.
        unsafe {
            HostState::release(NonNull::new_unchecked((*record).state.get()));
            drop(Box::from_raw(record));
        }
    }
}

/// A [`ProcessOwner`] with the type of its keep-alive value erased, so that one instantiation
/// of [`drive_owned`] serves every owner type instead of duplicating `drive_process` per type.
pub struct AnyProcessOwner {
    record: NonNull<core::ffi::c_void>,
    drop_record: unsafe fn(NonNull<core::ffi::c_void>),
}

impl<K: 'static> ProcessOwner<K> {
    /// Erase the type of the keep-alive value. The drop behavior is unchanged.
    pub fn erase(self) -> AnyProcessOwner {
        /// # Safety
        /// `record` must have come from `ProcessOwner::<K>::new` for this exact `K`,
        /// and this function must be called at most once for it.
        unsafe fn drop_record<K: 'static>(record: NonNull<core::ffi::c_void>) {
            // The pointer came from `ProcessOwner::<K>::new`, through `erase`.
            drop(ProcessOwner::<K>(record.cast()))
        }
        let this = core::mem::ManuallyDrop::new(self);
        AnyProcessOwner {
            record: this.0.cast(),
            drop_record: drop_record::<K>,
        }
    }
}

impl Drop for AnyProcessOwner {
    fn drop(&mut self) {
        // SAFETY: the pointer and the drop glue came from `ProcessOwner::erase`.
        unsafe { (self.drop_record)(self.record) }
    }
}

/// Drive the program's coroutine to its end, then free what it ran on. The coroutine is
/// dropped before `owner`, on every path, including the future being dropped while the
/// program is suspended: the temporary that holds it is the newest object of this
/// function, so it is the first to be dropped.
pub async fn drive_owned(coroutine: Coroutine, owner: AnyProcessOwner) -> i32 {
    let status = drive_process(coroutine).await;
    drop(owner);
    status
}

/// Run `entry` as a process on the calling task, as a future that resolves to its exit
/// status, without spawning a task for it.
///
/// The coroutine runs inside the task that polls the future, which is how the process
/// learns its identity. Dropping the future before it completes cancels the program: its
/// coroutine stack is freed, with no destructor run on it, and everything it held is
/// released. Without a `launch` the program gets no process capability.
///
/// # Safety
/// `entry` obeys the startup lifetime rules, fits on `stack`, and returns without
/// unwinding. The future is polled on the executor that created it and keeps all native
/// execution on it.
pub unsafe fn start_process<K: 'static>(
    stack: Stack,
    owner: ProcessOwner<K>,
    entry: NativeEntry,
    launch: Option<ProcessLaunch>,
) -> impl Future<Output = i32> + 'static {
    let coroutine = unsafe { Coroutine::new(stack, owner.entry(entry, launch)) };
    drive_owned(coroutine, owner.erase())
}
