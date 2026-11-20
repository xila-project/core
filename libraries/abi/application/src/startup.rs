//! Startup capabilities borrowed by a linked native C application.
use crate::{Directory, File, Heap, Process, ProcessHost, XILA_API, XilaApi};
use alloc::{string::String, vec::Vec};
use core::{ffi::c_char, ptr};
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
    /// Executable's launch name. Empty for a linked entry without a process.
    pub program_name: XilaString,
}

/// What the entry call borrows and the process owner releases when it ends. Kept in
/// one place so `xila_process_exit` can release it before it abandons the stack.
#[derive(Default)]
pub(crate) struct Resources {
    /// Strings that `arguments` points into.
    pub(crate) _owned: Vec<String>,
    pub(crate) arguments: Vec<XilaString>,
    pub(crate) program_name: String,
    pub(crate) standard_input: Option<File>,
    pub(crate) standard_output: Option<File>,
    pub(crate) standard_error: Option<File>,
    pub(crate) home: Option<Directory>,
}

/// Invoke a linked C entry, then close its granted streams. The process owner
/// provides the heap and drops it after all native execution ends. Keep the heap
/// owner outside any coroutine stack that can be abandoned.
///
/// The entry gets no process capability; a loader that starts native processes
/// uses [`crate::ProcessLaunch::run`].
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
    let resources = Resources {
        _owned: Vec::new(),
        program_name: String::new(),
        arguments: arguments
            .iter()
            .map(|argument| XilaString {
                data: argument.as_ptr().cast(),
                length: argument.len(),
            })
            .collect(),
        standard_input,
        standard_output,
        standard_error,
        home,
    };
    unsafe { enter(entry, heap, None, resources) }
}

/// Build the startup data over `resources`, call the entry and drop the resources
/// when it returns.
///
/// This frame, and every frame between it and a call to `xila_process_exit`, is
/// abandoned by that call without running a destructor. That is sound only because
/// the host never calls back into C with host state (a lock, guard or borrow) on the
/// stack: whenever C runs, the only host frames below it are this one and the
/// coroutine's own. Explicit process exit empties `resources` after stopping peers.
/// Worker callbacks are separate coroutine entries and carry no host-owned guards.
///
/// # Safety
/// Same as [`run_main`].
pub(crate) unsafe fn enter(
    entry: unsafe extern "C" fn(*const XilaStartup) -> i32,
    heap: &mut Heap,
    process: Option<(&'static dyn ProcessHost, TaskIdentifier)>,
    mut resources: Resources,
) -> i32 {
    // Every access below goes through this pointer, so `xila_process_exit` can empty
    // the resources while the entry runs.
    let resources_pointer: *mut Resources = &mut resources;
    let process = process.map(|(host, task)| Process {
        host,
        task,
        resources: resources_pointer,
    });
    // SAFETY: the pointer addresses the live local above.
    let resources = unsafe { &mut *resources_pointer };
    let startup = XilaStartup {
        size: core::mem::size_of::<XilaStartup>() as u32,
        api: &XILA_API,
        heap,
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
        process: process.as_ref().map_or(ptr::null(), ptr::from_ref),
        program_name: XilaString {
            data: resources.program_name.as_ptr().cast(),
            length: resources.program_name.len(),
        },
    };
    if !startup.process.is_null() {
        crate::threads::bind_process(startup.process);
    }
    let status = unsafe { entry(&startup) };
    if !startup.process.is_null() {
        crate::threads::stop_workers();
    }
    status
}
