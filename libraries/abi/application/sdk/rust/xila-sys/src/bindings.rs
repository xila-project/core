// Generated from the native application ABI. Do not edit.

use core::ffi::{c_char, c_void};
/// Native application entry points, in slot order. The header precedes slot zero.
/// Networking slots return Unsupported when the runtime omits networking, and so does
/// the graphics slot when it omits graphics.
#[repr(C)]
pub struct XilaApi {
    /// Packed major and minor Xila version: `(major << 16) | minor`.
    pub version: u32,
    /// Total size in bytes, including this header.
    pub size: u32,
    pub directory_open: unsafe extern "C" fn(
        parent: *const Directory,
        path: *const c_char,
        path_length: usize,
        rights: u32,
        directory: *mut *mut Directory,
    ) -> Status,
    pub directory_close: unsafe extern "C" fn(directory: *mut Directory) -> Status,
    pub directory_create: unsafe extern "C" fn(
        directory: *const Directory,
        path: *const c_char,
        length: usize,
    ) -> Status,
    pub directory_remove: unsafe extern "C" fn(
        directory: *const Directory,
        path: *const c_char,
        length: usize,
        flags: u32,
    ) -> Status,
    pub directory_rename: unsafe extern "C" fn(
        from: *const Directory,
        from_path: *const c_char,
        from_length: usize,
        to: *const Directory,
        to_path: *const c_char,
        to_length: usize,
    ) -> Status,
    pub file_open: unsafe extern "C" fn(
        directory: *const Directory,
        path: *const c_char,
        path_length: usize,
        flags: u32,
        file: *mut *mut File,
    ) -> Status,
    pub file_read: unsafe extern "C" fn(
        file: *mut File,
        buffer: *mut c_void,
        size: usize,
        count: *mut usize,
    ) -> Status,
    pub file_write: unsafe extern "C" fn(
        file: *mut File,
        buffer: *const c_void,
        size: usize,
        count: *mut usize,
    ) -> Status,
    pub file_seek: unsafe extern "C" fn(
        file: *mut File,
        offset: i64,
        whence: u32,
        position: *mut u64,
    ) -> Status,
    pub file_flush: unsafe extern "C" fn(file: *mut File) -> Status,
    pub file_close: unsafe extern "C" fn(file: *mut File) -> Status,
    pub file_set_size: unsafe extern "C" fn(file: *mut File, size: u64) -> Status,
    pub file_get_statistics: unsafe extern "C" fn(
        file: *mut File,
        output: *mut Statistics,
    ) -> Status,
    pub directory_get_statistics: unsafe extern "C" fn(
        directory: *const Directory,
        path: *const c_char,
        length: usize,
        flags: u32,
        output: *mut Statistics,
    ) -> Status,
    pub directory_iterator_open: unsafe extern "C" fn(
        directory: *const Directory,
        path: *const c_char,
        length: usize,
        output: *mut *mut DirectoryIterator,
    ) -> Status,
    pub directory_iterator_next: unsafe extern "C" fn(
        iterator: *mut DirectoryIterator,
        name_buffer: *mut c_char,
        capacity: usize,
        name_length: *mut usize,
        kind: *mut u32,
    ) -> Status,
    pub directory_iterator_rewind: unsafe extern "C" fn(
        iterator: *mut DirectoryIterator,
    ) -> Status,
    pub directory_iterator_close: unsafe extern "C" fn(
        iterator: *mut DirectoryIterator,
    ) -> Status,
    pub time_monotonic: unsafe extern "C" fn(nanoseconds: *mut u64) -> Status,
    pub time_realtime: unsafe extern "C" fn(nanoseconds: *mut i64) -> Status,
    pub network_close: unsafe extern "C" fn(network: *mut Network) -> Status,
    pub tcp_connect: unsafe extern "C" fn(
        network: *const Network,
        address: *const NetworkAddress,
        timeout_ms: u32,
        connection: *mut *mut Tcp,
    ) -> Status,
    pub tcp_listen: unsafe extern "C" fn(
        network: *const Network,
        address: *const NetworkAddress,
        listener: *mut *mut TcpListener,
    ) -> Status,
    pub tcp_accept: unsafe extern "C" fn(
        listener: *mut TcpListener,
        connection: *mut *mut Tcp,
    ) -> Status,
    pub tcp_close: unsafe extern "C" fn(socket: *mut Tcp) -> Status,
    pub tcp_listener_close: unsafe extern "C" fn(socket: *mut TcpListener) -> Status,
    pub tcp_shutdown: unsafe extern "C" fn(socket: *mut Tcp, direction: u32) -> Status,
    pub tcp_send: unsafe extern "C" fn(
        socket: *mut Tcp,
        buffer: *const c_void,
        size: usize,
        count: *mut usize,
    ) -> Status,
    pub tcp_receive: unsafe extern "C" fn(
        socket: *mut Tcp,
        buffer: *mut c_void,
        size: usize,
        count: *mut usize,
    ) -> Status,
    pub tcp_local_address: unsafe extern "C" fn(
        socket: *const Tcp,
        address: *mut NetworkAddress,
    ) -> Status,
    pub tcp_peer_address: unsafe extern "C" fn(
        socket: *const Tcp,
        address: *mut NetworkAddress,
    ) -> Status,
    pub udp_bind: unsafe extern "C" fn(
        network: *const Network,
        address: *const NetworkAddress,
        socket: *mut *mut Udp,
    ) -> Status,
    pub udp_close: unsafe extern "C" fn(socket: *mut Udp) -> Status,
    pub udp_send_to: unsafe extern "C" fn(
        socket: *mut Udp,
        buffer: *const c_void,
        size: usize,
        address: *const NetworkAddress,
        count: *mut usize,
    ) -> Status,
    pub udp_receive_from: unsafe extern "C" fn(
        socket: *mut Udp,
        buffer: *mut c_void,
        size: usize,
        address: *mut NetworkAddress,
        count: *mut usize,
    ) -> Status,
    pub network_resolve: unsafe extern "C" fn(
        network: *const Network,
        host: *const c_char,
        host_length: usize,
        family: u32,
        addresses: *mut NetworkAddress,
        count: *mut usize,
    ) -> Status,
    /// Allocate uninitialized memory aligned to XILA_HEAP_ALIGNMENT.
    pub heap_allocate: unsafe extern "C" fn(
        heap: *mut Heap,
        size: usize,
        allocation: *mut *mut c_void,
    ) -> Status,
    /// Resize an allocation. Zero size frees the block and stores null.
    pub heap_reallocate: unsafe extern "C" fn(
        heap: *mut Heap,
        pointer: *mut c_void,
        size: usize,
        allocation: *mut *mut c_void,
    ) -> Status,
    /// Release an allocation from this heap. Null succeeds.
    pub heap_free: unsafe extern "C" fn(heap: *mut Heap, pointer: *mut c_void) -> Status,
    /// Read a clock's nominal resolution: XILA_CLOCK_MONOTONIC or XILA_CLOCK_REALTIME.
    /// Returns Unavailable when the host installed no clock.
    pub time_resolution: unsafe extern "C" fn(
        clock: u32,
        nanoseconds: *mut u64,
    ) -> Status,
    /// Suspend for at least the given nanoseconds on the executor's clock. A zero wait
    /// yields once inside a coroutine and returns at once outside one.
    pub time_sleep: unsafe extern "C" fn(nanoseconds: u64) -> Status,
    /// Fill a buffer from the host-granted entropy source. Zero size succeeds. The
    /// buffer contents are unspecified after a failure.
    pub random_fill: unsafe extern "C" fn(buffer: *mut c_void, size: usize) -> Status,
    /// Read the process's identifier.
    pub process_id: unsafe extern "C" fn(
        process: *const Process,
        id: *mut u32,
    ) -> Status,
    /// Read the identifier of the process that spawned this one, or zero (the root
    /// task) once that process has exited.
    pub process_parent_id: unsafe extern "C" fn(
        process: *const Process,
        id: *mut u32,
    ) -> Status,
    /// End the process with an exit status. Does not return on success; outside a
    /// native process coroutine it returns Unsupported.
    pub process_exit: unsafe extern "C" fn(
        process: *const Process,
        status: i32,
    ) -> Status,
    /// Start an executable below a directory capability as a child process.
    pub process_spawn: unsafe extern "C" fn(
        process: *const Process,
        directory: *const Directory,
        path: *const c_char,
        path_length: usize,
        spawn: *const Spawn,
        child: *mut *mut Child,
    ) -> Status,
    /// Collect a child's exit status. Only the spawning process may wait; flags:
    /// XILA_WAIT_NO_HANG.
    pub process_wait: unsafe extern "C" fn(
        process: *const Process,
        child: *mut Child,
        flags: u32,
        status: *mut i32,
    ) -> Status,
    /// Release a child handle. Null succeeds.
    pub process_child_close: unsafe extern "C" fn(child: *mut Child) -> Status,
    /// Read an environment variable; BufferTooSmall stores the required length.
    pub env_get: unsafe extern "C" fn(
        process: *const Process,
        name: *const c_char,
        name_length: usize,
        buffer: *mut c_char,
        capacity: usize,
        value_length: *mut usize,
    ) -> Status,
    /// Set an environment variable of this process.
    pub env_set: unsafe extern "C" fn(
        process: *const Process,
        name: *const c_char,
        name_length: usize,
        value: *const c_char,
        value_length: usize,
    ) -> Status,
    /// Remove an environment variable of this process. A missing one succeeds.
    pub env_remove: unsafe extern "C" fn(
        process: *const Process,
        name: *const c_char,
        name_length: usize,
    ) -> Status,
    /// Read a counted NAME=VALUE entry by index; NotFound marks the end.
    pub env_entry: unsafe extern "C" fn(
        process: *const Process,
        index: usize,
        buffer: *mut c_char,
        capacity: usize,
        entry_length: *mut usize,
    ) -> Status,
    /// Spawn a worker of the current process; stack_size zero chooses the default.
    pub thread_spawn: unsafe extern "C" fn(
        process: *const Process,
        stack_size: usize,
        entry: ThreadEntry,
        argument: *mut c_void,
        thread: *mut usize,
    ) -> Status,
    /// Collect the worker result. Only the thread that created it may join.
    pub thread_join: unsafe extern "C" fn(
        thread: usize,
        result: *mut *mut c_void,
    ) -> Status,
    pub thread_detach: unsafe extern "C" fn(thread: usize) -> Status,
    pub thread_yield: unsafe extern "C" fn() -> Status,
    pub thread_current: unsafe extern "C" fn(thread: *mut usize) -> Status,
    pub thread_key_create: unsafe extern "C" fn(key: *mut usize) -> Status,
    pub thread_key_delete: unsafe extern "C" fn(key: usize) -> Status,
    pub thread_tls_get: unsafe extern "C" fn(
        key: usize,
        value: *mut *mut c_void,
    ) -> Status,
    pub thread_tls_set: unsafe extern "C" fn(key: usize, value: *mut c_void) -> Status,
    /// Address of the SDK-reserved libc state, valid until the current thread ends.
    pub thread_libc: unsafe extern "C" fn(location: *mut *mut ThreadLibc) -> Status,
    pub mutex_init: unsafe extern "C" fn(mutex: *mut Mutex, flags: u32) -> Status,
    pub mutex_destroy: unsafe extern "C" fn(mutex: *mut Mutex) -> Status,
    /// A contended XILA_MUTEX_TRY returns WouldBlock.
    pub mutex_lock: unsafe extern "C" fn(mutex: *mut Mutex, flags: u32) -> Status,
    pub mutex_unlock: unsafe extern "C" fn(mutex: *mut Mutex) -> Status,
    pub condition_init: unsafe extern "C" fn(condition: *mut Condition) -> Status,
    pub condition_destroy: unsafe extern "C" fn(condition: *mut Condition) -> Status,
    /// Unlock and sleep atomically, then reacquire before returning. A null deadline
    /// means infinite; otherwise use absolute nanoseconds on the selected ABI clock.
    pub condition_wait: unsafe extern "C" fn(
        condition: *mut Condition,
        mutex: *mut Mutex,
        clock: u32,
        deadline: *const i64,
    ) -> Status,
    pub condition_notify: unsafe extern "C" fn(
        condition: *mut Condition,
        all: bool,
    ) -> Status,
    /// Report whether any worker is live, including the caller if it is a worker.
    pub thread_has_workers: unsafe extern "C" fn(live: *mut bool) -> Status,
    /// Begin terminal cleanup. The process driver will only resume the caller.
    pub thread_stop_others: unsafe extern "C" fn(process: *const Process) -> Status,
    /// End a worker with a join result. This returns only on failure.
    pub thread_exit: unsafe extern "C" fn(result: *mut c_void) -> Status,
    /// Wait for readiness across file, pipe, directory and socket handles.
    /// timeout_ms is relative; negative means infinite, zero probes once.
    pub poll_wait: unsafe extern "C" fn(
        entries: *mut PollEntry,
        count: usize,
        timeout_ms: i32,
        ready_count: *mut usize,
    ) -> Status,
    /// Transfer from an explicit offset without changing the shared cursor.
    pub file_read_at: unsafe extern "C" fn(
        file: *mut File,
        buffer: *mut c_void,
        size: usize,
        offset: u64,
        count: *mut usize,
    ) -> Status,
    /// Transfer to an explicit offset without changing the shared cursor.
    pub file_write_at: unsafe extern "C" fn(
        file: *mut File,
        buffer: *const c_void,
        size: usize,
        offset: u64,
        count: *mut usize,
    ) -> Status,
    /// Return an owned file that shares the original cursor and status flags.
    pub file_duplicate: unsafe extern "C" fn(
        file: *const File,
        output: *mut *mut File,
    ) -> Status,
    /// Query queued pipe bytes or get/set file flags using a uint32_t buffer.
    pub file_control: unsafe extern "C" fn(
        file: *mut File,
        command: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: usize,
    ) -> Status,
    /// Report whether an open file is an interactive terminal.
    pub file_is_terminal: unsafe extern "C" fn(
        file: *mut File,
        terminal: *mut bool,
    ) -> Status,
    /// Create the read and write handles of an unnamed pipe.
    /// The only supported flag is XILA_OPEN_NON_BLOCKING.
    pub pipe_create: unsafe extern "C" fn(
        process: *const Process,
        flags: u32,
        reader: *mut *mut File,
        writer: *mut *mut File,
    ) -> Status,
    /// Run one graphics call by its ID from `xila/graphics.h`. `arguments` holds
    /// `argument_count` words (at most seven; null at zero) and `result` receives the
    /// call's word or 32-bit result. The SDK's generated functions pack the words.
    pub graphics_call: unsafe extern "C" fn(
        process: *const Process,
        function: u32,
        arguments: *const usize,
        argument_count: usize,
        result: *mut c_void,
    ) -> Status,
    /// Read a child's process identifier, the value its `process_id` reports.
    pub process_child_id: unsafe extern "C" fn(
        child: *const Child,
        id: *mut u32,
    ) -> Status,
}
pub const XILA_RIGHT_READ: u32 = 1;
pub const XILA_RIGHT_WRITE: u32 = 2;
/// Opaque path capability. Native callers must serialize access and preserve ownership.
#[repr(C)]
pub struct Directory {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
/// Allow removing directories as well as files. Removal never follows the final link.
pub const XILA_REMOVE_DIRECTORY: u32 = 1;
pub const XILA_OPEN_READ: u32 = 1;
pub const XILA_OPEN_WRITE: u32 = 2;
pub const XILA_OPEN_CREATE: u32 = 4;
pub const XILA_OPEN_EXCLUSIVE: u32 = 8;
pub const XILA_OPEN_TRUNCATE: u32 = 16;
pub const XILA_OPEN_APPEND: u32 = 32;
pub const XILA_OPEN_NON_BLOCKING: u32 = 64;
pub const XILA_SEEK_START: u32 = 0;
pub const XILA_SEEK_CURRENT: u32 = 1;
pub const XILA_SEEK_END: u32 = 2;
/// Opaque owned file. No handle lookup or buffer allocation is required for I/O.
#[repr(C)]
pub struct File {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
pub const XILA_CONTROL_BYTES_AVAILABLE: u32 = 1;
pub const XILA_CONTROL_GET_FILE_FLAGS: u32 = 2;
pub const XILA_CONTROL_SET_FILE_FLAGS: u32 = 3;
pub const XILA_PIPE_CAPACITY: usize = 512;
/// Opaque owned listing with at most one pending VFS entry for buffer retries.
#[repr(C)]
pub struct DirectoryIterator {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
pub const XILA_STATISTICS_NO_FOLLOW: u32 = 1;
pub const XILA_KIND_FILE: u32 = 1;
pub const XILA_KIND_DIRECTORY: u32 = 2;
pub const XILA_KIND_BLOCK_DEVICE: u32 = 3;
pub const XILA_KIND_CHARACTER_DEVICE: u32 = 4;
pub const XILA_KIND_PIPE: u32 = 5;
pub const XILA_KIND_SOCKET: u32 = 6;
pub const XILA_KIND_SYMLINK: u32 = 7;
/// Versioned output: initialize size to sizeof(XilaStatistics) before each call.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Statistics {
    pub size: u32,
    pub kind: u32,
    pub permissions: u32,
    pub owner_user: u32,
    pub owner_group: u32,
    pub reserved: u32,
    pub file_size: u64,
    pub link_count: u64,
    pub inode: u64,
    pub access_nanoseconds: i64,
    pub modification_nanoseconds: i64,
    pub status_change_nanoseconds: i64,
}
pub const XILA_NETWORK_CONNECT: u32 = 1;
pub const XILA_NETWORK_LISTEN: u32 = 2;
pub const XILA_NETWORK_RESOLVE: u32 = 4;
pub const XILA_ADDRESS_IPV4: u32 = 4;
pub const XILA_ADDRESS_IPV6: u32 = 6;
pub const XILA_SHUTDOWN_WRITE: u32 = 1;
/// Fixed-width address. Bytes are in network order; integer fields are host order.
/// IPv4 uses the first four bytes and requires the remaining bytes to be zero.
/// Scope must be zero: Core does not support scoped IPv6 endpoints.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkAddress {
    pub family: u32,
    pub bytes: [u8; 16],
    pub port: u16,
    pub reserved: u16,
    pub scope_id: u32,
}
/// Host-granted network authority. Future host/port restrictions belong here.
#[repr(C)]
pub struct Network {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
/// Owned TCP connection. Its creation grants bidirectional I/O authority.
#[repr(C)]
pub struct Tcp {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
/// One pending connection, with no additional backlog. Accept replenishes the socket.
#[repr(C)]
pub struct TcpListener {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
/// Owned UDP socket. Sending additionally requires CONNECT on its parent capability.
#[repr(C)]
pub struct Udp {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
pub const XILA_POLL_IGNORE: u32 = 0;
pub const XILA_POLL_FILE: u32 = 1;
pub const XILA_POLL_DIRECTORY: u32 = 2;
pub const XILA_POLL_TCP: u32 = 3;
pub const XILA_POLL_TCP_LISTENER: u32 = 4;
pub const XILA_POLL_UDP: u32 = 5;
pub const XILA_POLL_IN: u16 = 1;
pub const XILA_POLL_OUT: u16 = 2;
pub const XILA_POLL_ERR: u16 = 4;
pub const XILA_POLL_HUP: u16 = 8;
pub const XILA_POLL_NVAL: u16 = 16;
/// One poll entry. Only IN and OUT are requested. ERR, HUP and NVAL are
/// reported regardless of the request. IGNORE entries always return zero.
/// A null file/directory/socket or unknown kind reports NVAL.
#[repr(C)]
pub struct PollEntry {
    /// Opaque object of the type named by kind.
    pub handle: *const c_void,
    pub kind: u32,
    pub events: u16,
    pub revents: u16,
}
/// `xila_process_wait` flag: return `WouldBlock` instead of waiting for a running child.
pub const XILA_WAIT_NO_HANG: u32 = 1;
/// Opaque capability of the running process: its identity, environment, children and
/// exit. The host grants it through `XilaStartup.process`; the pointer is borrowed for
/// the entry call and a process must not pass it to another process.
#[repr(C)]
pub struct Process {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
/// A spawned child, owned by the process that spawned it. It holds the child's
/// exit status until the owner waits for it, so a child that finished and was not
/// waited for costs one signal slot and no task. `xila_process_child_close`
/// releases it, waited for or not.
#[repr(C)]
pub struct Child {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
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
/// A native worker entry. Null is rejected.
pub type ThreadEntry = Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>;
pub const XILA_THREAD_STACK_MIN: usize = 2048;
pub const XILA_THREAD_STACK_MAX: usize = 64 * 1024;
pub const XILA_THREAD_KEYS_MAX: usize = 8;
pub const XILA_MUTEX_RECURSIVE: u32 = 1;
pub const XILA_MUTEX_TRY: u32 = 1;
/// SDK-reserved coroutine-local libc state. Other applications should use TLS keys.
#[repr(C)]
pub struct ThreadLibc {
    pub(crate) error: i32,
    pub(crate) strtok_next: *mut c_char,
    pub(crate) random_state: u64,
}
/// Process-local mutex. Zero initialize for a plain mutex, or use mutex_init.
/// Keep its address fixed from first use through destruction. The fields are private
/// to the host; copying a used mutex or sharing it across processes is invalid.
#[repr(C)]
pub struct Mutex {
    pub(crate) owner: usize,
    pub(crate) depth: u32,
    pub(crate) recursive: bool,
    pub(crate) waiters: *mut c_void,
}
impl Mutex {
    /// Initial storage, before any host operation.
    pub const ZERO: Self = Self {
        owner: 0,
        depth: 0,
        recursive: false,
        waiters: core::ptr::null_mut(),
    };
}
/// Process-local condition variable. Zero initialize or use condition_init.
/// Keep its address fixed from first use through destruction. The fields are private
/// to the host; copying a used condition or sharing it across processes is invalid.
#[repr(C)]
pub struct Condition {
    pub(crate) waiters: *mut c_void,
}
impl Condition {
    /// Initial storage, before any host operation.
    pub const ZERO: Self = Self {
        waiters: core::ptr::null_mut(),
    };
}
/// Monotonic clock identifier for `xila_time_resolution`.
pub const XILA_CLOCK_MONOTONIC: u32 = 0;
/// Realtime (Unix UTC) clock identifier for `xila_time_resolution`.
pub const XILA_CLOCK_REALTIME: u32 = 1;
/// Alignment guaranteed for every allocation, including zero-length requests.
#[cfg(target_arch = "arm")]
pub const XILA_HEAP_ALIGNMENT: usize = 8;
/// Alignment guaranteed for every allocation, including zero-length requests.
#[cfg(not(target_arch = "arm"))]
pub const XILA_HEAP_ALIGNMENT: usize = 16;
/// Opaque native heap capability. The process owner drops it after all native
/// execution ends. Keep ownership outside any stack that can be abandoned.
///
/// Native applications are trusted, so misuse detection is cheap rather than
/// hardened. Every block starts with a header whose tag is the heap's key XOR
/// the block address. Free and resize read the header that would precede the
/// pointer and reject it as `InvalidArgument` unless it is aligned and its tag
/// matches: an interior pointer or a double free (including one through a
/// pointer that a moving resize left stale) is caught, and so is a
/// foreign-heap pointer unless the two heaps' keys collide, which is
/// unlikely. Like a C allocator, this reads through the pointer, so it needs
/// readable memory below it. It cannot detect a freed block whose memory was
/// reused by a later allocation of this heap, which then identifies the new
/// block. Metadata lives in the block, so no operation allocates it and a
/// shrinking resize never fails because of it.
#[repr(C)]
pub struct Heap {
    _opaque: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}
/// Native status values mirror the unified error kinds one-to-one. Zero is success.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Status(pub u32);
#[allow(non_upper_case_globals)]
impl Status {
    pub const Ok: Self = Self(0);
    pub const InvalidArgument: Self = Self(0x0101);
    pub const InvalidData: Self = Self(0x0102);
    pub const PermissionDenied: Self = Self(0x0103);
    pub const OperationNotPermitted: Self = Self(0x0104);
    pub const InputOutput: Self = Self(0x0105);
    pub const OutOfMemory: Self = Self(0x0106);
    pub const WouldBlock: Self = Self(0x0107);
    pub const ResourceBusy: Self = Self(0x0108);
    pub const Interrupted: Self = Self(0x0109);
    pub const TimedOut: Self = Self(0x010a);
    pub const Unsupported: Self = Self(0x010b);
    pub const Overflow: Self = Self(0x010c);
    pub const WriteZero: Self = Self(0x010d);
    pub const NoBuffers: Self = Self(0x010e);
    pub const NotFound: Self = Self(0x010f);
    pub const AlreadyExists: Self = Self(0x0110);
    pub const InvalidFlags: Self = Self(0x0111);
    pub const InvalidState: Self = Self(0x0112);
    pub const NotImplemented: Self = Self(0x0113);
    pub const ExecFormat: Self = Self(0x0114);
    pub const NoChild: Self = Self(0x0115);
    pub const ArgumentListTooLong: Self = Self(0x0116);
    pub const Deadlock: Self = Self(0x0117);
    pub const BadDescriptor: Self = Self(0x0201);
    pub const InvalidPath: Self = Self(0x0202);
    pub const MissingPathComponent: Self = Self(0x0203);
    pub const MissingFinalEntry: Self = Self(0x0204);
    pub const NameTooLong: Self = Self(0x0205);
    pub const SymlinkLoop: Self = Self(0x0206);
    pub const NotADirectory: Self = Self(0x0207);
    pub const IsADirectory: Self = Self(0x0208);
    pub const DirectoryNotEmpty: Self = Self(0x0209);
    pub const NoSpace: Self = Self(0x020a);
    pub const FileTooLarge: Self = Self(0x020b);
    pub const ProcessFileLimit: Self = Self(0x020c);
    pub const SystemFileLimit: Self = Self(0x020d);
    pub const CrossDevice: Self = Self(0x020e);
    pub const PathEscapesRoot: Self = Self(0x020f);
    pub const CorruptData: Self = Self(0x0210);
    pub const MissingAttribute: Self = Self(0x0211);
    pub const StaleHandle: Self = Self(0x0212);
    pub const ReadOnlyFileSystem: Self = Self(0x0213);
    pub const InvalidSeek: Self = Self(0x0214);
    pub const Unavailable: Self = Self(0x0301);
    pub const NoSuchTask: Self = Self(0x0302);
    pub const InappropriateIoControl: Self = Self(0x0303);
    pub const ConnectionRefused: Self = Self(0x0401);
    pub const ConnectionReset: Self = Self(0x0402);
    pub const ConnectionAborted: Self = Self(0x0403);
    pub const NotConnected: Self = Self(0x0404);
    pub const AlreadyConnected: Self = Self(0x0405);
    pub const AddressInUse: Self = Self(0x0406);
    pub const AddressNotAvailable: Self = Self(0x0407);
    pub const DestinationRequired: Self = Self(0x0408);
    pub const HostUnreachable: Self = Self(0x0409);
    pub const NetworkUnreachable: Self = Self(0x040a);
    pub const NetworkDown: Self = Self(0x040b);
    pub const BrokenPipe: Self = Self(0x040c);
    pub const MessageTooLarge: Self = Self(0x040d);
    pub const DatagramTruncated: Self = Self(0x040e);
    pub const SocketOperationUnsupported: Self = Self(0x040f);
    pub const ProtocolUnsupported: Self = Self(0x0410);
    pub const AddressFamilyUnsupported: Self = Self(0x0411);
    pub const InProgress: Self = Self(0x0412);
    pub const AlreadyInProgress: Self = Self(0x0413);
    pub const ProtocolError: Self = Self(0x0414);
    pub const NotASocket: Self = Self(0x0415);
    pub const InvalidDnsName: Self = Self(0x0501);
    pub const UnsupportedDnsQuery: Self = Self(0x0502);
    pub const NameNotResolved: Self = Self(0x0503);
    pub const ResolverTemporaryFailure: Self = Self(0x0504);
    pub const ResolverFailure: Self = Self(0x0505);
    pub const End: Self = Self(0x0701);
    pub const BufferTooSmall: Self = Self(0x0702);
}
/// Major version of the Xila release this ABI belongs to. Native images
/// record `(XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR` in their ABI note.
pub const XILA_VERSION_MAJOR: u32 = 0;
/// Minor version of the Xila release this ABI belongs to.
pub const XILA_VERSION_MINOR: u32 = 1;
