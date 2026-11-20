//! Native process capabilities: identity, exit, spawn, wait and environment.
//!
//! A native process is the Core task that runs a native entry on a stackful
//! coroutine. The task owns the identity, the parent link, the user and group
//! and the environment. This module adds no process table: it reads and writes
//! the task manager, and starts children through the host's executable path.
use crate::{
    Directory, File, Heap, Resources, Status, XILA_RIGHT_READ, XilaString, canonical, enter,
    is_in_coroutine, path_input, suspend_dynamic,
};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    ffi::c_char,
    future::Future,
    pin::{Pin, pin},
    ptr, slice, str,
};
use file_system::{IntoIoErrorKind, PathOwned};
use task::{EnvironmentVariable, JoinHandle, TaskIdentifier};
use virtual_file_system::{File as VfsFile, VirtualFileSystem};

/// `xila_process_wait` flag: return `WouldBlock` instead of waiting for a running child.
pub const XILA_WAIT_NO_HANG: u32 = 1;

/// The exit status of a child that could not start its entry.
///
/// An exit status is any `int32_t`, so no value is outside the range a program can
/// return: this one is indistinguishable from a program that calls `exit(-1)`. A
/// parent that needs to tell them apart must have the child report success some
/// other way, such as a line on its standard output.
const START_FAILURE: i32 = -1;

/// The native status of a task manager failure: the canonical I/O kind mapping.
fn task_status(error: task::Error) -> Status {
    error.into_io_error_kind().into()
}

/// A directory capability granted to a child as its home.
pub struct HomeGrant {
    pub path: PathOwned,
    pub rights: u8,
}

/// What the host needs to start a child: the validated, owned result of
/// `xila_process_spawn`'s arguments. The three standard streams are duplicates of the
/// parent's, owned by the request.
#[non_exhaustive]
pub struct SpawnRequest {
    /// Absolute, canonical VFS path of the executable, inside the caller's directory capability.
    pub path: PathOwned,
    pub arguments: Vec<String>,
    /// The child's whole environment, already validated, or `None` to give it a copy
    /// of the parent's. A later variable overrides an earlier one of the same name.
    pub environment: Option<Vec<EnvironmentVariable>>,
    pub standard_input: VfsFile<'static>,
    pub standard_output: VfsFile<'static>,
    pub standard_error: VfsFile<'static>,
    pub home: Option<HomeGrant>,
}

/// The result of starting a child: its join handle, which carries its exit status.
pub type SpawnFuture = Pin<Box<dyn Future<Output = Result<JoinHandle<isize>, Status>>>>;

/// The host services behind a process capability. Core implements them for its
/// executable context, so a spawned child goes through `executable::execute`.
pub trait ProcessHost: Sync {
    fn task_manager(&self) -> &'static task::Manager;
    fn virtual_file_system(&self) -> &'static VirtualFileSystem;
    /// Start the executable as a child task on the caller's executor. The child's
    /// coroutine must enter its program through [`ProcessLaunch::run`].
    fn spawn(&'static self, request: SpawnRequest) -> SpawnFuture;
}

/// What a child's coroutine needs to become a process, carried from the spawn
/// request to the loader that starts its entry.
pub struct ProcessLaunch {
    pub host: &'static dyn ProcessHost,
    /// Replaces the environment the task copied from its parent.
    pub environment: Option<Vec<EnvironmentVariable>>,
    /// Becomes the child's home directory capability, bound to the child's task.
    pub home: Option<HomeGrant>,
}

impl ProcessLaunch {
    /// Run a native entry as a process, on the calling coroutine. The coroutine runs
    /// inside the process's task, which is how the process learns its identity.
    ///
    /// Returns the entry's exit status, or -1 if the process could not be set up:
    /// the coroutine is not on a registered task or the task is gone.
    ///
    /// Every resource the process owns is held by the arguments or by this call's
    /// own state. `xila_process_exit` releases the startup grants before
    /// ending the process. A worker that exits the process cancels main as
    /// described in #731.
    /// The caller must keep no other owned object live above this call.
    ///
    /// # Safety
    /// Same contract as [`crate::run_main`].
    pub unsafe fn run(
        self,
        entry: unsafe extern "C" fn(*const crate::XilaStartup) -> i32,
        heap: &mut Heap,
        arguments: Vec<String>,
        standard_input: Option<File>,
        standard_output: Option<File>,
        standard_error: Option<File>,
    ) -> i32 {
        let Self {
            host,
            environment,
            home,
        } = self;
        let manager = host.task_manager();
        let mut identity = Err(Status::Unavailable);
        // `environment` is consumed here: nothing owned remains live when C starts.
        suspend_dynamic(pin!(async {
            identity = async {
                let task = manager.try_get_current_task_identifier().await?;
                if let Some(environment) = environment {
                    manager
                        .replace_environment_variables(task, environment)
                        .await?;
                }
                Ok::<_, task::Error>((task, manager.get_name(task).await?))
            }
            .await
            .map_err(task_status);
        }));
        let Ok((task, program_name)) = identity else {
            return START_FAILURE;
        };
        let home = home.map(|grant| {
            Directory::granted(host.virtual_file_system(), task, grant.path, grant.rights)
        });
        let mut pointers = Vec::with_capacity(arguments.len());
        for argument in &arguments {
            pointers.push(XilaString {
                data: argument.as_ptr().cast(),
                length: argument.len(),
            });
        }
        let resources = Resources {
            _owned: arguments,
            program_name,
            arguments: pointers,
            standard_input,
            standard_output,
            standard_error,
            home,
        };
        unsafe { enter(entry, heap, Some((host, task)), resources) }
    }
}

/// Opaque capability of the running process: its identity, environment, children and
/// exit. The host grants it through `XilaStartup.process`; the pointer is borrowed for
/// the entry call and a process must not pass it to another process.
pub struct Process {
    pub(crate) host: &'static dyn ProcessHost,
    pub(crate) task: TaskIdentifier,
    pub(crate) resources: *mut Resources,
}

/// A spawned child, owned by the process that spawned it. It holds the child's
/// exit status until the owner waits for it, so a child that finished and was not
/// waited for costs one signal slot and no task. `xila_process_child_close`
/// releases it, waited for or not.
pub struct Child {
    owner: TaskIdentifier,
    handle: Option<JoinHandle<isize>>,
}

/// Arguments of `xila_process_spawn`. Set `size` to `sizeof(XilaSpawn)`. All
/// pointers are borrowed for the call.
#[repr(C)]
pub struct Spawn {
    pub size: u32,
    /// Reserved, must be zero.
    pub flags: u32,
    /// Arguments of the child, with no implicit program name. A null array has count zero.
    pub arguments: *const XilaString,
    pub argument_count: usize,
    /// The child's whole environment as `NAME=VALUE` strings, a later entry overriding an
    /// earlier one. A null array gives the child a copy of the parent's environment
    /// and requires count zero. An array with count zero gives it an empty one.
    pub environment: *const XilaString,
    pub environment_count: usize,
    /// Streams the child gets as duplicates of these. All three are required.
    pub standard_input: *const File,
    pub standard_output: *const File,
    pub standard_error: *const File,
    /// Optional home directory capability the child gets a copy of.
    pub home: *const Directory,
}

/// A task identifier as the ABI reports it: `u16` on 32-bit targets, already `u32` on 64-bit ones.
#[allow(clippy::unnecessary_cast)]
pub(crate) fn identifier(task: TaskIdentifier) -> u32 {
    task.into_inner() as u32
}

/// Check that the capability is the running task's, which is how a process cannot act
/// through another process's capability.
async fn is_current(process: &Process) -> Result<(), Status> {
    match process
        .host
        .task_manager()
        .try_get_current_task_identifier()
        .await
    {
        Ok(task) if task == process.task => Ok(()),
        _ => Err(Status::InvalidArgument),
    }
}

fn this<'a>(process: *const Process) -> Result<&'a Process, Status> {
    // SAFETY: the caller guarantees a non-null pointer is a live capability.
    unsafe { process.as_ref() }.ok_or(Status::InvalidArgument)
}

/// Read the process's identifier.
/// # Safety
/// Process must be a live capability and the output aligned and writable.
pub unsafe extern "C" fn xila_process_id(process: *const Process, id: *mut u32) -> Status {
    let result = this(process).and_then(|process| {
        if id.is_null() {
            return Err(Status::InvalidArgument);
        }
        unsafe { id.write(identifier(process.task)) };
        Ok(())
    });
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Read the identifier of the process that spawned this one. A process whose parent
/// has exited is adopted by the root task, identifier zero.
/// # Safety
/// Process must be a live capability and the output aligned and writable.
pub unsafe extern "C" fn xila_process_parent_id(process: *const Process, id: *mut u32) -> Status {
    let result = this(process).and_then(|process| {
        if id.is_null() {
            return Err(Status::InvalidArgument);
        }
        let mut parent = Err(Status::Unavailable);
        suspend_dynamic(pin!(async {
            parent = process
                .host
                .task_manager()
                .get_parent(process.task)
                .await
                .map_err(task_status);
        }));
        unsafe { id.write(identifier(parent?)) };
        Ok(())
    });
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Stop every peer, release the startup grants, and exit without unwinding.
/// Stopped peers follow stackful's cancellation policy (#731), including
/// leaked stack resources. The loader releases the process heap and image.
///
/// Both main and worker entries run without host-owned guards in callback
/// frames, so abandoning the calling stack does not skip their cleanup.
/// # Safety
/// Process must be the running process's capability.
pub unsafe extern "C" fn xila_process_exit(process: *const Process, status: i32) -> Status {
    let process = match this(process) {
        Ok(process) => process,
        Err(error) => return error,
    };
    if !is_in_coroutine() {
        return Status::Unsupported;
    }
    if let Err(error) = crate::threads::prepare_exit(ptr::from_ref(process)) {
        return error;
    }
    let mut current = Err(Status::Unavailable);
    suspend_dynamic(pin!(async {
        current = is_current(process).await;
    }));
    if let Err(error) = current {
        return error;
    }
    // SAFETY: the pointer is the live startup state on main's stack. The
    // driver will never poll another coroutine, even if cleanup suspends.
    unsafe { *process.resources = Resources::default() };
    crate::exit_coroutine(status)
}

/// A UTF-8 name that can key an environment variable.
unsafe fn env_name<'a>(name: *const c_char, length: usize) -> Result<&'a str, Status> {
    let name = unsafe { path_input(name, length)? };
    if name.is_empty() || name.contains(['=', '\0']) {
        return Err(Status::InvalidArgument);
    }
    Ok(name)
}

/// UTF-8 text without NUL. A null pointer is valid only at length zero.
unsafe fn env_value<'a>(value: *const c_char, length: usize) -> Result<&'a str, Status> {
    if length == 0 {
        return Ok("");
    }
    let value = unsafe { path_input(value, length)? };
    if value.contains('\0') {
        return Err(Status::InvalidArgument);
    }
    Ok(value)
}

/// Read an environment variable. The value is copied without a terminator; the
/// length is stored on success and on `BufferTooSmall`, which leaves the buffer
/// unchanged. A null buffer is allowed at capacity zero. A missing variable returns
/// `NotFound`.
/// # Safety
/// Process must be a live capability. Name must be readable for its length, the buffer
/// writable for its capacity and the length output aligned and writable.
pub unsafe extern "C" fn xila_env_get(
    process: *const Process,
    name: *const c_char,
    name_length: usize,
    buffer: *mut c_char,
    capacity: usize,
    value_length: *mut usize,
) -> Status {
    let result = (|| {
        let process = this(process)?;
        if value_length.is_null()
            || capacity > isize::MAX as usize
            || (capacity != 0 && buffer.is_null())
        {
            return Err(Status::InvalidArgument);
        }
        let name = unsafe { env_name(name, name_length)? };
        let mut found = Err(Status::Unavailable);
        suspend_dynamic(pin!(async {
            found = process
                .host
                .task_manager()
                .get_environment_variable(process.task, name)
                .await
                .map_err(|error| match error {
                    // The getter's only way to say that the variable does not exist.
                    task::Error::InvalidEnvironmentVariable => Status::NotFound,
                    error => task_status(error),
                });
        }));
        let variable = found?;
        let value = variable.get_value();
        unsafe { value_length.write(value.len()) };
        if value.len() > capacity {
            return Err(Status::BufferTooSmall);
        }
        // SAFETY: a nonzero length implies a nonzero capacity, hence a non-null buffer.
        unsafe { ptr::copy_nonoverlapping(value.as_ptr(), buffer.cast(), value.len()) };
        Ok(())
    })();
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Set an environment variable of this process, replacing any value. Children
/// spawned afterwards inherit a copy; earlier children and the parent are unaffected.
/// A name is non-empty UTF-8 without `=` or NUL, a value is UTF-8 without NUL and a
/// null value pointer is valid at length zero.
/// # Safety
/// Process must be a live capability and the strings readable for their lengths.
pub unsafe extern "C" fn xila_env_set(
    process: *const Process,
    name: *const c_char,
    name_length: usize,
    value: *const c_char,
    value_length: usize,
) -> Status {
    let result = (|| {
        let process = this(process)?;
        let name = unsafe { env_name(name, name_length)? };
        let value = unsafe { env_value(value, value_length)? };
        let mut set = Err(Status::Unavailable);
        suspend_dynamic(pin!(async {
            set = process
                .host
                .task_manager()
                .set_environment_variable(process.task, name, value)
                .await
                .map_err(task_status);
        }));
        set
    })();
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Remove an environment variable of this process. A missing variable succeeds.
/// # Safety
/// Process must be a live capability and the name readable for its length.
pub unsafe extern "C" fn xila_env_remove(
    process: *const Process,
    name: *const c_char,
    name_length: usize,
) -> Status {
    let result = (|| {
        let process = this(process)?;
        let name = unsafe { env_name(name, name_length)? };
        let mut removed = Err(Status::Unavailable);
        suspend_dynamic(pin!(async {
            removed = process
                .host
                .task_manager()
                .remove_environment_variable(process.task, name)
                .await
                .map_err(task_status);
        }));
        removed
    })();
    result.map_or_else(|error| error, |()| Status::Ok)
}

/// Borrow a counted array of strings as a slice, which is empty at count zero.
unsafe fn entries<'a>(array: *const XilaString, count: usize) -> Result<&'a [XilaString], Status> {
    if count == 0 {
        return Ok(&[]);
    }
    if array.is_null() || count > isize::MAX as usize / size_of::<XilaString>() {
        return Err(Status::InvalidArgument);
    }
    Ok(unsafe { slice::from_raw_parts(array, count) })
}

/// Borrow one string as UTF-8 text.
unsafe fn text<'a>(entry: &XilaString) -> Result<&'a str, Status> {
    if entry.length == 0 {
        return Ok("");
    }
    if entry.data.is_null() || entry.length > isize::MAX as usize {
        return Err(Status::InvalidArgument);
    }
    str::from_utf8(unsafe { slice::from_raw_parts(entry.data.cast(), entry.length) })
        .map_err(|_| Status::InvalidArgument)
}

/// Copy the argument strings of a spawn.
unsafe fn arguments(array: *const XilaString, count: usize) -> Result<Vec<String>, Status> {
    let entries = unsafe { entries(array, count)? };
    let mut arguments = Vec::with_capacity(entries.len());
    for entry in entries {
        arguments.push(String::from(unsafe { text(entry)? }));
    }
    Ok(arguments)
}

/// Validate the `NAME=VALUE` entries of an explicit environment once, so the host
/// receives them as the task manager stores them.
unsafe fn environment(
    array: *const XilaString,
    count: usize,
) -> Result<Vec<EnvironmentVariable>, Status> {
    let entries = unsafe { entries(array, count)? };
    let mut variables = Vec::with_capacity(entries.len());
    for entry in entries {
        match unsafe { text(entry)? }.split_once('=') {
            Some((name, value))
                if !name.is_empty() && !name.contains('\0') && !value.contains('\0') =>
            {
                variables.push(EnvironmentVariable::new(name, value));
            }
            _ => return Err(Status::InvalidArgument),
        }
    }
    Ok(variables)
}

/// Start an executable as a child of this process and return its handle.
///
/// The path names a file below the directory capability, which needs the READ right.
/// The host checks the execute permission as the calling process's user, and the child
/// runs as the same user. The child gets an owned duplicate of each standard stream, a
/// copy of the home capability if any, the given arguments, and a copy of this
/// process's environment unless the request gives an explicit one. Spawning
/// suspends the caller while the host reads the executable. Failures map to the
/// usual statuses: `NotFound`, `PermissionDenied`, `ExecFormat` for an image the host
/// cannot run, and `OutOfMemory` when the task table is full. Task manager errors use
/// the canonical I/O kind mapping (`NoSpawnerAvailable` is `Unavailable`,
/// `InvalidSpawnerIdentifier` is `InvalidArgument`). The
/// process must be the running process's capability, otherwise the call returns
/// `InvalidArgument`. The handle is written only on success.
/// # Safety
/// Process and directory must be live capabilities, the path readable for its length, the
/// request and everything it points to valid for the call, and the output writable.
pub unsafe extern "C" fn xila_process_spawn(
    process: *const Process,
    directory: *const Directory,
    path: *const c_char,
    path_length: usize,
    spawn: *const Spawn,
    child: *mut *mut Child,
) -> Status {
    let result = (|| {
        let process = this(process)?;
        if directory.is_null() || spawn.is_null() || child.is_null() {
            return Err(Status::InvalidArgument);
        }
        // SAFETY: checked non-null above; the caller guarantees the objects are live.
        let (directory, spawn) = unsafe { (&*directory, &*spawn) };
        if (spawn.size as usize) < size_of::<Spawn>()
            || spawn.flags != 0
            || spawn.standard_input.is_null()
            || spawn.standard_output.is_null()
            || spawn.standard_error.is_null()
            || (spawn.environment.is_null() && spawn.environment_count != 0)
        {
            return Err(Status::InvalidArgument);
        }
        if u32::from(directory.rights) & XILA_RIGHT_READ == 0 {
            return Err(Status::PermissionDenied);
        }
        let path = canonical(
            directory.path.as_str(),
            unsafe { path_input(path, path_length)? },
            true,
        )?;
        let arguments = unsafe { arguments(spawn.arguments, spawn.argument_count)? };
        let environment = if spawn.environment.is_null() {
            None
        } else {
            Some(unsafe { environment(spawn.environment, spawn.environment_count)? })
        };
        let home = unsafe { spawn.home.as_ref() }.map(|home| HomeGrant {
            path: home.path.clone(),
            rights: home.rights,
        });
        // SAFETY: the caller guarantees the three streams are live files.
        let streams = unsafe {
            [
                &*spawn.standard_input,
                &*spawn.standard_output,
                &*spawn.standard_error,
            ]
        };
        let host = process.host;
        let parent = process.task;
        // Allocated before the child starts, so a started child is never left unowned.
        let mut owned = Box::new(Child {
            owner: parent,
            handle: None,
        });
        let mut started = Err(Status::Unavailable);
        suspend_dynamic(pin!(async {
            started = async {
                is_current(process).await?;
                // One duplication site for the three streams. A failure drops, and so
                // closes, the duplicates already made.
                let mut duplicates = [None, None, None];
                for (duplicate, stream) in duplicates.iter_mut().zip(streams) {
                    *duplicate = Some(stream.inner.duplicate().await?);
                }
                let [
                    Some(standard_input),
                    Some(standard_output),
                    Some(standard_error),
                ] = duplicates
                else {
                    return Err(Status::InvalidState);
                };
                host.spawn(SpawnRequest {
                    path,
                    arguments,
                    environment,
                    standard_input,
                    standard_output,
                    standard_error,
                    home,
                })
                .await
            }
            .await;
        }));
        owned.handle = Some(started?);
        Ok(owned)
    })();
    match result {
        Ok(value) => {
            unsafe { child.write(Box::into_raw(value)) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Wait for a child to exit and collect its exit status, which reaps it: a later wait on
/// the same handle returns `NoChild`. Only the process that spawned the child may
/// wait. Inside a coroutine the call suspends until the child exits, so other tasks keep
/// running. With `XILA_WAIT_NO_HANG`, or outside a coroutine where nothing can suspend,
/// a running child returns `WouldBlock`. The status output is optional and written
/// only on success.
///
/// A child that exited and is not waited for keeps its status in its handle and holds
/// no task. If its parent exits first, the root task adopts it.
/// # Safety
/// Process and child must be live and the status output, if any, aligned and writable.
pub unsafe extern "C" fn xila_process_wait(
    process: *const Process,
    child: *mut Child,
    flags: u32,
    status: *mut i32,
) -> Status {
    let result = (|| {
        let process = this(process)?;
        if child.is_null() || flags & !XILA_WAIT_NO_HANG != 0 {
            return Err(Status::InvalidArgument);
        }
        // SAFETY: checked non-null above; the caller guarantees the handle is live and unshared.
        let child = unsafe { &mut *child };
        if child.owner != process.task {
            return Err(Status::NoChild);
        }
        let finished = child.handle.as_ref().ok_or(Status::NoChild)?.is_finished();
        if !finished && (flags & XILA_WAIT_NO_HANG != 0 || !is_in_coroutine()) {
            return Err(Status::WouldBlock);
        }
        let handle = child.handle.take().ok_or(Status::NoChild)?;
        let mut exit = 0;
        suspend_dynamic(pin!(async {
            exit = handle.join().await;
        }));
        // Statuses wider than 32 bits come only from non-native children.
        Ok(exit as i32)
    })();
    match result {
        Ok(value) => {
            if !status.is_null() {
                unsafe { status.write(value) };
            }
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Release a child handle, waited for or not. A running child keeps running and its
/// status is discarded when it ends. Null succeeds.
/// # Safety
/// A non-null child must be owned and released exactly once.
pub unsafe extern "C" fn xila_process_child_close(child: *mut Child) -> Status {
    if !child.is_null() {
        drop(unsafe { Box::from_raw(child) });
    }
    Status::Ok
}
