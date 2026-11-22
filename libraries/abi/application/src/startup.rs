//! Startup capabilities borrowed by a linked native C application.
use crate::{
    Directory, File, Heap, Network, Process, ProcessHost, XILA_API, XilaApi, registry::Registry,
};
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
/// Fields are only ever appended: check `size` before reading a field that a
/// later Xila version added, such as `network`.
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
    /// Executable's launch name. Empty for a linked entry without a process.
    pub program_name: XilaString,
    /// Network authority granted to the process: connect, listen and resolve. Null when
    /// the host has no network stack, the firmware was built without networking, or the
    /// application is not a process. Appended after `program_name`, so images built
    /// before it existed still start.
    pub network: *const Network,
}

/// What the entry call borrows and the process owner releases when it ends. Kept in
/// one place so `xila_process_exit` can release it before it abandons the stack, and so
/// the host can release it when it cancels the process.
pub(crate) struct Resources {
    /// The handles the program opened itself and has not closed: files, directories,
    /// listings, children and sockets. See [`crate::registry`].
    pub(crate) handles: Registry,
    /// Strings that `arguments` points into.
    pub(crate) _owned: Vec<String>,
    pub(crate) arguments: Vec<XilaString>,
    /// Bytes that `XilaStartup::program_name` points into.
    pub(crate) program_name: String,
    pub(crate) standard_input: Option<File>,
    pub(crate) standard_output: Option<File>,
    pub(crate) standard_error: Option<File>,
    pub(crate) home: Option<Directory>,
    /// The network capability behind `XilaStartup::network`. It owns no socket: the
    /// sockets the program opened are in `handles`.
    #[cfg(feature = "network")]
    pub(crate) network: Option<Network>,
    /// The process's graphics objects, allocated by its first graphics call.
    #[cfg(feature = "graphics")]
    pub(crate) graphics: Option<alloc::boxed::Box<graphics::call::State>>,
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
/// It owns everything the program holds that the kernel or the host must release: the
/// arguments, the standard streams, the home directory, the graphics objects and a registry
/// of the handles the program opened itself. The program's frames own none of it, so
/// [`HostState::release`] can close it all without running any of the program's code,
/// whether the program returned, exited or was cancelled.
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
            handles: Registry::new(),
            arguments: argument_pointers(&mut arguments.iter().map(String::as_str)),
            _owned: arguments,
            program_name: String::new(),
            standard_input,
            standard_output,
            standard_error,
            home: None,
            #[cfg(feature = "network")]
            network: None,
            #[cfg(feature = "graphics")]
            graphics: None,
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
                program_name: XilaString {
                    data: ptr::null(),
                    length: 0,
                },
                network: ptr::null(),
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
    /// Close and free what the process owns, once. Releasing an empty set does nothing, so
    /// a return from the entry, `xila_process_exit` and the host that cancels the process
    /// can all call it.
    ///
    /// Each kind is emptied before the next is closed, and each stream is closed where it
    /// is and taken out afterwards, so a close that suspends and is then cancelled leaves
    /// everything not yet closed in the state, for the host to release. The order is the
    /// reverse of how a program acquires them: the handles it opened, its graphics objects,
    /// its streams, its home directory, then its strings.
    ///
    /// # Safety
    /// The pointer addresses live resources, no reference to them is in use and no code of
    /// the program that owns them runs, or ever will, while this does.
    #[inline(never)]
    pub(crate) unsafe fn release(resources: *mut Self) {
        // SAFETY: the caller guarantees the pointer is valid and unaliased.
        unsafe {
            (*resources).handles.release_all();
            // The process's graphics objects go next, under the graphics lock, which the
            // process waits for by suspending. The state stays in the resources until
            // it is dropped, so a coroutine cancelled in that wait leaves it for the host.
            #[cfg(feature = "graphics")]
            {
                let state = (*resources).graphics.as_mut().map(|state| &raw mut **state);
                if let Some(state) = state {
                    crate::graphics::release(state);
                }
                drop((*resources).graphics.take());
            }
            for stream in [
                &mut (*resources).standard_input,
                &mut (*resources).standard_output,
                &mut (*resources).standard_error,
            ] {
                // Closed in place and then taken: a close that is cancelled while it waits
                // leaves the stream in the state, for the host to close again.
                if let Some(file) = stream {
                    let _ = file.inner.as_synchronous_file_mut().close_in_place();
                }
                drop(stream.take());
            }
            drop((*resources).home.take());
            #[cfg(feature = "network")]
            {
                (*resources).network = None;
            }
            (*resources).arguments = Vec::new();
            (*resources)._owned = Vec::new();
            (*resources).program_name = String::new();
        }
    }
}

impl HostState {
    /// How many handles the program has opened and not closed: files, directories,
    /// listings, children and sockets. It counts what a cancellation would close, and
    /// excludes the standard streams and the home directory, which are not the program's
    /// to close.
    pub fn open_handles(&self) -> usize {
        self.resources.handles.len()
    }

    /// Close and free everything the program still holds: the handles it opened, its
    /// graphics objects, its standard streams, its home directory and its strings. A
    /// program that returned or exited has released them already, so this does nothing
    /// then; one that was cancelled, or never ran, still holds them.
    ///
    /// This is how a host cancels a program without running any of its code: drop the
    /// coroutine first (which discards its frames), then release the state, then free it.
    /// Closing may wait for the file system and the graphics lock, which blocks the
    /// calling task as `task::block_on` does when no coroutine is running.
    ///
    /// # Safety
    /// `state` is live and at the address the program ran with, and no code of the program
    /// runs, or ever will, while this does.
    pub unsafe fn release(state: NonNull<Self>) {
        // SAFETY: the caller guarantees the state is live and unaliased.
        unsafe { Resources::release(&raw mut (*state.as_ptr()).resources) }
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
        handles: Registry::new(),
        _owned: Vec::new(),
        program_name: String::new(),
        arguments: argument_pointers(&mut arguments.iter().copied()),
        standard_input,
        standard_output,
        standard_error,
        home,
        #[cfg(feature = "network")]
        network: None,
        #[cfg(feature = "graphics")]
        graphics: None,
    });
    unsafe { enter(NonNull::from(&mut state), entry, NonNull::from(heap), None) }
}

/// The startup's borrowed network capability, or null without one.
fn network_pointer(resources: &Resources) -> *const Network {
    #[cfg(feature = "network")]
    {
        resources
            .network
            .as_ref()
            .map_or(ptr::null(), ptr::from_ref)
    }
    #[cfg(not(feature = "network"))]
    {
        let _ = resources;
        ptr::null()
    }
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
        // A process gets the network authority of its host; nothing else does.
        #[cfg(feature = "network")]
        if let Some((host, _)) = process
            && let Some(manager) = host.network_manager()
        {
            resources.network = Network::new(
                manager,
                crate::XILA_NETWORK_CONNECT
                    | crate::XILA_NETWORK_LISTEN
                    | crate::XILA_NETWORK_RESOLVE,
            )
            .ok();
        }
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
            program_name: XilaString {
                data: resources.program_name.as_ptr().cast(),
                length: resources.program_name.len(),
            },
            network: network_pointer(resources),
        };
        &raw const (*state).startup
    };
    let process = unsafe { (*startup).process };
    let managed = !process.is_null();
    if managed {
        crate::threads::bind_process(process);
    }
    // Every thread of the program registers the handles it opens here.
    crate::threads::bind_registry(unsafe { &raw const (*resources).handles });
    let status = unsafe { entry(startup) };
    if managed {
        crate::threads::stop_workers();
    }
    unsafe { Resources::release(resources) };
    status
}
