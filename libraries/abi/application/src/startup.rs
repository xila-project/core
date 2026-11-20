//! Startup capabilities borrowed by a linked native C application.
use crate::{Directory, File, Heap, Process, ProcessHost, XILA_API, XilaApi};
use alloc::{string::String, vec::Vec};
use core::{ffi::c_char, ptr, ptr::NonNull};
use task::TaskIdentifier;

/// UTF-8 bytes, not NUL-terminated. A null data pointer is valid only at length zero.
#[repr(C)]
pub struct XilaString {
    pub data: *const c_char,
    pub length: usize,
}

/// Host-owned startup data, valid only for the duration of the entry call.
/// The API is always present. Capability pointers may be null when not granted;
/// the process capability is null for an application that is not a process, such
/// as a built-in one linked into the firmware, whose process calls fail with
/// InvalidArgument. A null argument array has argument_count zero.
/// Applications must not close, retain or transfer these objects; closing them is
/// undefined behavior. Objects opened by the application must still be closed.
#[repr(C)]
pub struct XilaStartup {
    pub size: u32,
    pub api: *const XilaApi,
    pub heap: *mut Heap,
    pub arguments: *const XilaString,
    pub argument_count: usize,
    pub standard_input: *mut File,
    pub standard_output: *mut File,
    pub standard_error: *mut File,
    pub home: *mut Directory,
    pub process: *const Process,
}

/// What the entry call borrows and the process owner releases when it ends. Kept in
/// one place so `xila_process_exit` can release it before it abandons the stack.
#[derive(Default)]
pub(crate) struct Resources {
    /// Strings that `arguments` points into.
    pub(crate) _owned: Vec<String>,
    pub(crate) arguments: Vec<XilaString>,
    pub(crate) standard_input: Option<File>,
    pub(crate) standard_output: Option<File>,
    pub(crate) standard_error: Option<File>,
    pub(crate) home: Option<Directory>,
}

/// Everything the host keeps for one native run: the startup block the entry
/// borrows, the process capability and the resources both point into.
///
/// It belongs to the host side of the task, not to the program's coroutine: the
/// loader allocates it once, next to the heap and the image, and the coroutine
/// entry receives only a pointer to it. A program that writes past its own frames
/// therefore cannot damage the state `xila_process_exit`, the SDK's heap access and
/// the standard error stream behind `write(2)` depend on, and a host that stops
/// resuming a coroutine can still reach what the coroutine owned.
///
/// The startup block and the capability point into the state, so it must not move
/// once a run has begun: the allocation that holds it stays where it is until the
/// run has ended for good. Building it, and moving it into that allocation, are
/// free to move it, because nothing points into it yet.
///
/// It owns the arguments and streams until a run starts. A loader that stops the state
/// from being dropped with its allocation, to leave it alone after a cancellation, must
/// still drop it when the run never starts: nothing else holds those streams.
pub struct HostState {
    startup: XilaStartup,
    process: Option<Process>,
    pub(crate) resources: Resources,
}

/// The startup array over `arguments`, which must outlive it. One instance serves
/// both callers.
fn argument_pointers(arguments: &mut dyn ExactSizeIterator<Item = &str>) -> Vec<XilaString> {
    let mut pointers = Vec::with_capacity(arguments.len());
    for argument in arguments {
        pointers.push(XilaString {
            data: argument.as_ptr().cast(),
            length: argument.len(),
        });
    }
    pointers
}

impl HostState {
    /// The arguments and the standard streams of a program that has not started.
    pub fn new(
        arguments: Vec<String>,
        standard_input: Option<File>,
        standard_output: Option<File>,
        standard_error: Option<File>,
    ) -> Self {
        Self::from_resources(Resources {
            arguments: argument_pointers(&mut arguments.iter().map(String::as_str)),
            _owned: arguments,
            standard_input,
            standard_output,
            standard_error,
            home: None,
        })
    }

    fn from_resources(resources: Resources) -> Self {
        Self {
            // Filled in by `enter`, which knows the heap and the capability.
            startup: XilaStartup {
                size: 0,
                api: ptr::null(),
                heap: ptr::null_mut(),
                arguments: ptr::null(),
                argument_count: 0,
                standard_input: ptr::null_mut(),
                standard_output: ptr::null_mut(),
                standard_error: ptr::null_mut(),
                home: ptr::null_mut(),
                process: ptr::null(),
            },
            process: None,
            resources,
        }
    }

    /// Run a linked entry that gets no process capability and cannot exit early,
    /// then drop the resources. The state belongs to the caller, as for
    /// [`crate::ProcessLaunch::run`].
    ///
    /// # Safety
    /// Same as [`run_main`], and `state` and `heap` stay at their addresses and live
    /// until native execution has permanently ended.
    pub unsafe fn run_linked(
        state: NonNull<Self>,
        entry: unsafe extern "C" fn(*const XilaStartup) -> i32,
        heap: NonNull<Heap>,
    ) -> i32 {
        unsafe { enter(state, entry, heap, None) }
    }
}

impl Resources {
    /// Drop what the process owns, once. Dropping an empty set does nothing, so a
    /// return from the entry and `xila_process_exit` can both call it.
    ///
    /// # Safety
    /// The pointer addresses live resources and no reference to them is in use.
    #[inline(never)]
    pub(crate) unsafe fn release(resources: *mut Self) {
        // SAFETY: the caller guarantees the pointer is valid. `take` writes the empty
        // set before anything is dropped, so a destructor that suspends (a `File` closes
        // through the VFS) and is then cancelled leaves an empty state behind, never a
        // half-dropped one that a later release would drop again.
        drop(unsafe { core::mem::take(&mut *resources) });
    }
}

/// Invoke a linked C entry on the calling stack, then close its granted streams.
/// The process owner provides the heap and drops it after all native execution
/// ends.
///
/// The entry gets no process capability and cannot exit early; the host state sits
/// in this frame. A loader that starts native processes keeps the state off the
/// stack: see [`HostState`] and [`crate::ProcessLaunch::run`].
/// # Safety
/// Entry must obey the startup lifetime and ownership rules, serialize object
/// access, and never unwind. The heap must remain live until native execution
/// has permanently ended.
pub unsafe fn run_main(
    entry: unsafe extern "C" fn(*const XilaStartup) -> i32,
    heap: &mut Heap,
    arguments: &[&str],
    standard_input: Option<File>,
    standard_output: Option<File>,
    standard_error: Option<File>,
    home: Option<Directory>,
) -> i32 {
    let mut state = HostState::from_resources(Resources {
        _owned: Vec::new(),
        arguments: argument_pointers(&mut arguments.iter().copied()),
        standard_input,
        standard_output,
        standard_error,
        home,
    });
    unsafe { enter(NonNull::from(&mut state), entry, NonNull::from(heap), None) }
}

/// Build the startup data in `state`, call the entry and release the resources
/// when it returns. Nothing the entry or `xila_process_exit` reaches is stored in
/// this frame: they find the state through pointers into `state`.
///
/// Whichever way the run ends, the resources are dropped exactly once: here when the
/// entry returns, or by `xila_process_exit`, which empties them and abandons this
/// frame and every frame between it and the exit. That is sound only because the
/// host never calls back into C with host state (a lock, guard or borrow) on the
/// stack: whenever C runs, the only host frames below it are this one and the
/// coroutine's own. Worker callbacks are separate coroutine entries and carry no
/// host-owned guards.
///
/// # Safety
/// Same as [`run_main`], and `state` and `heap` stay at their addresses and live
/// until native execution has permanently ended.
pub(crate) unsafe fn enter(
    state: NonNull<HostState>,
    entry: unsafe extern "C" fn(*const XilaStartup) -> i32,
    heap: NonNull<Heap>,
    process: Option<(&'static dyn ProcessHost, TaskIdentifier)>,
) -> i32 {
    let state = state.as_ptr();
    // Every access goes through pointers into `state`, so `xila_process_exit` can
    // empty the resources while the entry runs.
    let resources: *mut Resources = unsafe { &raw mut (*state).resources };
    let startup = unsafe {
        (*state).process = process.map(|(host, task)| Process {
            host,
            task,
            resources,
        });
        let resources = &mut *resources;
        (*state).startup = XilaStartup {
            size: core::mem::size_of::<XilaStartup>() as u32,
            api: &XILA_API,
            heap: heap.as_ptr(),
            arguments: if resources.arguments.is_empty() {
                ptr::null()
            } else {
                resources.arguments.as_ptr()
            },
            argument_count: resources.arguments.len(),
            standard_input: resources
                .standard_input
                .as_mut()
                .map_or(ptr::null_mut(), ptr::from_mut),
            standard_output: resources
                .standard_output
                .as_mut()
                .map_or(ptr::null_mut(), ptr::from_mut),
            standard_error: resources
                .standard_error
                .as_mut()
                .map_or(ptr::null_mut(), ptr::from_mut),
            home: resources
                .home
                .as_mut()
                .map_or(ptr::null_mut(), ptr::from_mut),
            process: (*state).process.as_ref().map_or(ptr::null(), ptr::from_ref),
        };
        &raw const (*state).startup
    };
    let process = unsafe { (*startup).process };
    let managed = !process.is_null();
    if managed {
        crate::threads::bind_process(process);
    }
    let status = unsafe { entry(startup) };
    if managed {
        crate::threads::stop_workers();
    }
    unsafe { Resources::release(resources) };
    status
}
