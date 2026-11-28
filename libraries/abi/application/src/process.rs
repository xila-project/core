//! Native process capabilities: identity, exit, spawn, wait and environment.
//!
//! A native process is the Core task that runs a native entry on a stackful
//! coroutine. The task owns the identity, the parent link, the user and group
//! and the environment. This module adds no process table: it reads and writes
//! the task manager, and starts children through the host's executable path.
use crate::{
    Directory, File, Heap, HostState, Resources, Status, XILA_RIGHT_READ, XilaString, enter,
    is_in_coroutine, path_input,
    registry::{self, Handle},
    suspend_dynamic, suspend_on_cancellable,
};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{
    ffi::c_char,
    future::Future,
    pin::{Pin, pin},
    ptr::{self, NonNull},
    slice, str,
};
use file_system::{IntoIoErrorKind, PathOwned, Statistics};
use task::{EnvironmentVariable, JoinHandle, TaskIdentifier};
use virtual_file_system::{File as VfsFile, VirtualFileSystem};

/// `xila_process_wait` flag: return `WouldBlock` instead of waiting for a running child.
pub const XILA_WAIT_NO_HANG: u32 = 1;
/// `XilaSpawn.flags`: give the child the right to connect (and to send datagrams).
/// The value is `XILA_NETWORK_CONNECT`'s.
pub const XILA_SPAWN_NETWORK_CONNECT: u32 = 1;
/// `XilaSpawn.flags`: give the child the right to listen and bind. The value is
/// `XILA_NETWORK_LISTEN`'s.
pub const XILA_SPAWN_NETWORK_LISTEN: u32 = 2;
/// `XilaSpawn.flags`: give the child the right to resolve names. The value is
/// `XILA_NETWORK_RESOLVE`'s.
pub const XILA_SPAWN_NETWORK_RESOLVE: u32 = 4;
const SPAWN_NETWORK: u32 =
    XILA_SPAWN_NETWORK_CONNECT | XILA_SPAWN_NETWORK_LISTEN | XILA_SPAWN_NETWORK_RESOLVE;

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
pub enum HomeGrant {
    /// Grant the user's recorded home if it exists.
    User { rights: u8 },
    /// The host grants a directory by absolute path.
    Path { path: PathOwned, rights: u8 },
    /// A parent passes on a directory it already holds.
    Open(Directory),
}

/// What the host needs to start a child: the validated, owned result of
/// `xila_process_spawn`'s arguments. The three standard streams are duplicates of the
/// parent's, owned by the request.
#[non_exhaustive]
pub struct SpawnRequest {
    /// File name supplied by the caller, used as the task name.
    pub name: String,
    /// An executable opened within the caller's directory capability.
    pub executable: VfsFile<'static>,
    /// Metadata for the opened executable, including its execute permissions.
    pub statistics: Statistics,
    pub arguments: Vec<String>,
    /// The child's whole environment, already validated, or `None` to give it a copy
    /// of the parent's. A later variable overrides an earlier one of the same name.
    pub environment: Option<Vec<EnvironmentVariable>>,
    pub standard_input: VfsFile<'static>,
    pub standard_output: VfsFile<'static>,
    pub standard_error: VfsFile<'static>,
    pub home: Option<HomeGrant>,
    /// The child's network rights, already checked to be a subset of the parent's.
    /// Zero grants no network.
    pub network: u32,
}

/// The result of starting a child: its join handle, which carries its exit status, and
/// its task identifier, which is the child's process identifier.
pub type SpawnFuture =
    Pin<Box<dyn Future<Output = Result<(JoinHandle<isize>, TaskIdentifier), Status>>>>;

/// The host services behind a process capability. Core implements them for its
/// executable context, so a spawned child goes through `executable::execute`.
pub trait ProcessHost: Sync {
    fn task_manager(&self) -> &'static task::Manager;
    fn virtual_file_system(&self) -> &'static VirtualFileSystem;
    /// The graphics manager the process draws on, or `None` when the system has
    /// no display. `xila_graphics_call` reports Unavailable without one.
    #[cfg(feature = "graphics")]
    fn graphics_manager(&self) -> Option<&'static graphics::Manager> {
        None
    }
    /// The network stack the process's sockets use, or `None` when the system has none.
    /// A process gets `XilaStartup.network` only when its launch granted network rights
    /// ([`ProcessLaunch::network`]) and this returns a manager.
    #[cfg(feature = "network")]
    fn network_manager(&self) -> Option<&'static network::Manager> {
        None
    }
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
    /// The network rights the process gets (`XILA_NETWORK_CONNECT`, `_LISTEN`,
    /// `_RESOLVE`). Zero, the default for every launch, grants no network: nothing is
    /// inherited, and a parent can only pass a subset of its own rights to a child.
    pub network: u32,
}

impl ProcessLaunch {
    /// Run a native entry as a process, on the calling coroutine, with the startup
    /// data in `state`. The coroutine runs inside the process's task, which is how
    /// the process learns its identity.
    ///
    /// Returns the entry's exit status, or -1 if process setup fails.
    ///
    /// Every resource the process owns is held by `state`, which belongs to the
    /// host side of the task, not to this coroutine's stack: the call keeps only
    /// pointers to it. The setup future owns the home grant and environment
    /// until the process starts. A cancellation drops that future in place.
    /// Returning from the entry, a failed setup and `xila_process_exit` release
    /// the state's resources once before the process ends. After cancellation,
    /// the host releases those resources with [`HostState::release`].
    ///
    /// # Safety
    /// Same contract as [`crate::run_main`], and `state` and `heap` stay at their
    /// addresses and live until native execution has permanently ended. The caller
    /// must keep no other owned object live above this call.
    pub unsafe fn run(
        self,
        state: NonNull<HostState>,
        entry: unsafe extern "C" fn(*const crate::XilaStartup) -> i32,
        heap: NonNull<Heap>,
    ) -> i32 {
        let Self {
            host,
            environment,
            home,
            network,
        } = self;
        let manager = host.task_manager();
        let resources = unsafe { &raw mut (*state.as_ptr()).resources };
        // The future owns the environment and home grant while it waits.
        let identity = suspend_on_cancellable(async {
            let task = manager
                .try_get_current_task_identifier()
                .await
                .map_err(task_status)?;
            let program_name = manager.get_name(task).await.map_err(task_status)?;
            if let Some(environment) = environment {
                manager
                    .replace_environment_variables(task, environment)
                    .await
                    .map_err(task_status)?;
            }
            let home = match home {
                Some(HomeGrant::User { rights }) => {
                    let user = manager.get_user(task).await.map_err(task_status)?;
                    let vfs = host.virtual_file_system();
                    if let Ok(path) = vfs.get_user_home(user).await {
                        match Directory::new_async(vfs, task, path.as_str(), u32::from(rights))
                            .await
                        {
                            Ok(directory) => Some(directory),
                            Err(Status::NotFound) => None,
                            Err(error) => return Err(error),
                        }
                    } else {
                        None
                    }
                }
                Some(HomeGrant::Open(mut directory)) => {
                    directory.task = task;
                    Some(directory)
                }
                Some(HomeGrant::Path { path, rights }) => Some(
                    Directory::new_async(
                        host.virtual_file_system(),
                        task,
                        path.as_str(),
                        u32::from(rights),
                    )
                    .await?,
                ),
                None => None,
            };
            Ok::<_, Status>((task, program_name, home))
        });
        let Ok((task, program_name, home)) = identity else {
            // SAFETY: nothing has started that could use the resources.
            unsafe { Resources::release(resources) };
            return START_FAILURE;
        };
        // SAFETY: the caller guarantees the state is live; nothing else uses it yet.
        unsafe {
            (*resources).program_name = program_name;
            (*resources).home = home;
            // Only an explicit grant gives the process network authority.
            #[cfg(feature = "network")]
            if network != 0
                && let Some(manager) = host.network_manager()
            {
                (*resources).network = crate::Network::new(manager, network).ok();
            }
            #[cfg(not(feature = "network"))]
            let _ = network;
            enter(state, entry, heap, Some((host, task)))
        }
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

impl Process {
    /// The network rights this process was granted, zero without a network capability.
    fn network_rights(&self) -> u32 {
        #[cfg(feature = "network")]
        {
            // SAFETY: the resources live as long as the process capability.
            unsafe { (*self.resources).network.as_ref() }
                .map_or(0, |network| u32::from(network.rights))
        }
        #[cfg(not(feature = "network"))]
        {
            0
        }
    }
}

/// A spawned child, owned by the process that spawned it. It holds the child's
/// exit status until the owner waits for it, so a child that finished and was not
/// waited for costs one signal slot and no task. `xila_process_child_close`
/// releases it, waited for or not.
pub struct Child {
    owner: TaskIdentifier,
    /// The child's process identifier, known once it started.
    id: u32,
    handle: Option<JoinHandle<isize>>,
}

impl Handle for Child {
    unsafe fn close(raw: *mut Self) {
        // SAFETY: the registry holds only live handles that `xila_process_spawn` made.
        let _ = unsafe { xila_process_child_close(raw) };
    }
}

/// Arguments of `xila_process_spawn`. Set `size` to `sizeof(XilaSpawn)`. All
/// pointers are borrowed for the call.
#[repr(C)]
pub struct Spawn {
    pub size: u32,
    /// The network rights the child gets (`XILA_SPAWN_NETWORK_*`). Zero gives it no
    /// network. Each right must be one the parent holds: a child never gets more than
    /// its parent. Other bits are reserved and must be zero.
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

pub(crate) fn this<'a>(process: *const Process) -> Result<&'a Process, Status> {
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

/// Stop every peer, release the process's resources, and exit without unwinding.
/// Stopped peers are never resumed: their frames are discarded and their stacks freed
/// when the process's driver ends, as for a cancelled process. The loader then releases
/// the process heap and image.
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
    // SAFETY: the pointer is the live host state, which the coroutine's frames do
    // not hold. The driver will never poll another coroutine, even if cleanup suspends.
    unsafe { Resources::release(process.resources) };
    // The startup block lives in the released state: nothing may find it any more.
    crate::threads::bind_startup(ptr::null());
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

/// A caller-owned environment output buffer and length pointer.
struct EnvOutput {
    buffer: *mut c_char,
    capacity: usize,
    length: *mut usize,
}

impl EnvOutput {
    fn new(buffer: *mut c_char, capacity: usize, length: *mut usize) -> Result<Self, Status> {
        if length.is_null() || capacity > isize::MAX as usize || (capacity != 0 && buffer.is_null())
        {
            return Err(Status::InvalidArgument);
        }
        Ok(Self {
            buffer,
            capacity,
            length,
        })
    }

    /// The caller must have provided writable output pointers.
    unsafe fn write(self, bytes: &[u8]) -> Result<(), Status> {
        unsafe { self.length.write(bytes.len()) };
        if bytes.len() > self.capacity {
            return Err(Status::BufferTooSmall);
        }
        if !bytes.is_empty() {
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), self.buffer.cast(), bytes.len()) };
        }
        Ok(())
    }
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
        let output = EnvOutput::new(buffer, capacity, value_length)?;
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
        unsafe { output.write(variable.get_value().as_bytes()) }
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

/// Copy the zero-based environment entry as `NAME=VALUE` without a terminator.
/// Output follows the same rules as `xila_env_get`. `NotFound` marks the end.
/// Indices can change when the environment is mutated.
/// # Safety
/// Process must be a live capability, the buffer writable for its capacity and
/// the length output aligned and writable.
pub unsafe extern "C" fn xila_env_entry(
    process: *const Process,
    index: usize,
    buffer: *mut c_char,
    capacity: usize,
    entry_length: *mut usize,
) -> Status {
    let result = (|| {
        let process = this(process)?;
        let output = EnvOutput::new(buffer, capacity, entry_length)?;
        let mut entry = Err(Status::Unavailable);
        suspend_dynamic(pin!(async {
            entry = process
                .host
                .task_manager()
                .get_environment_variable_at(process.task, index)
                .await
                .map_err(task_status);
        }));
        let entry = entry?.ok_or(Status::NotFound)?;
        unsafe { output.write(entry.get_raw().to_bytes()) }
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
            || spawn.flags & !SPAWN_NETWORK != 0
            || spawn.standard_input.is_null()
            || spawn.standard_output.is_null()
            || spawn.standard_error.is_null()
            || (spawn.environment.is_null() && spawn.environment_count != 0)
        {
            return Err(Status::InvalidArgument);
        }
        if directory.task != process.task
            || !core::ptr::eq(directory.vfs, process.host.virtual_file_system())
        {
            return Err(Status::InvalidArgument);
        }
        // SAFETY: The caller supplies readable path bytes for path_length.
        let path = unsafe { directory.relative_path(path, path_length, XILA_RIGHT_READ, true)? };
        // A parent can pass on only network rights it holds itself.
        let network = spawn.flags & SPAWN_NETWORK;
        if network & !process.network_rights() != 0 {
            return Err(Status::PermissionDenied);
        }
        let arguments = unsafe { arguments(spawn.arguments, spawn.argument_count)? };
        let environment = if spawn.environment.is_null() {
            None
        } else {
            Some(unsafe { environment(spawn.environment, spawn.environment_count)? })
        };
        // SAFETY: A non-null home points to a live directory for this call, per the caller contract.
        let home = unsafe { spawn.home.as_ref() };
        if home.is_some_and(|home| {
            home.task != process.task || !core::ptr::eq(home.vfs, directory.vfs)
        }) {
            return Err(Status::InvalidArgument);
        }
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
        // Registered before the child starts: a call cancelled while it waits for the host
        // leaves the handle to the registry, and a started child is never left unowned.
        let owned = registry::register(Child {
            owner: parent,
            id: 0,
            handle: None,
        });
        // The future owns the duplicated streams and the request, which a cancellation
        // drops in place.
        let started = suspend_on_cancellable(async move {
            is_current(process).await?;
            let (executable, statistics) =
                directory.root.open_executable(directory.task, path).await?;
            let name = String::from(path.get_file_name().ok_or(Status::InvalidArgument)?);
            let home = home.map(|home| HomeGrant::Open(home.duplicate()));
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
                name,
                executable,
                statistics,
                arguments,
                environment,
                standard_input,
                standard_output,
                standard_error,
                home,
                network,
            })
            .await
        });
        match started {
            Ok((handle, task)) => {
                // SAFETY: the handle was just allocated and is not shared yet.
                unsafe {
                    (*owned).id = identifier(task);
                    (*owned).handle = Some(handle);
                }
                Ok(owned)
            }
            Err(error) => {
                // SAFETY: as above.
                unsafe { xila_process_child_close(owned) };
                Err(error)
            }
        }
    })();
    match result {
        Ok(value) => {
            unsafe { child.write(value) };
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
        // The future owns the handle, which a cancellation drops in place: the child runs on
        // and its status is discarded, as for a child handle that is closed.
        let exit = suspend_on_cancellable(handle.join());
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

/// Read the process identifier of a child: the value its own `xila_process_id`
/// reports. It stays readable after the child exits or is waited for, until the
/// handle is closed. Core may reuse the identifier once the child's task is gone.
/// # Safety
/// The child must be a live handle and the output aligned and writable.
pub unsafe extern "C" fn xila_process_child_id(child: *const Child, id: *mut u32) -> Status {
    if child.is_null() || id.is_null() {
        return Status::InvalidArgument;
    }
    // SAFETY: checked non-null above; the caller guarantees both are live and aligned.
    unsafe { id.write((*child).id) };
    Status::Ok
}

/// Release a child handle, waited for or not. A running child keeps running and its
/// status is discarded when it ends. Null succeeds.
/// # Safety
/// A non-null child must be owned and released exactly once.
pub unsafe extern "C" fn xila_process_child_close(child: *mut Child) -> Status {
    if !child.is_null() {
        // SAFETY: the caller guarantees the handle is owned and live.
        drop(unsafe { registry::reclaim(child) });
    }
    Status::Ok
}
