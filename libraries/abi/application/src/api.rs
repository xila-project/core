use crate::*;
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
    pub file_get_statistics:
        unsafe extern "C" fn(file: *mut File, output: *mut Statistics) -> Status,
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
    pub directory_iterator_rewind: unsafe extern "C" fn(iterator: *mut DirectoryIterator) -> Status,
    pub directory_iterator_close: unsafe extern "C" fn(iterator: *mut DirectoryIterator) -> Status,
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
    pub tcp_accept:
        unsafe extern "C" fn(listener: *mut TcpListener, connection: *mut *mut Tcp) -> Status,
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
    pub tcp_local_address:
        unsafe extern "C" fn(socket: *const Tcp, address: *mut NetworkAddress) -> Status,
    pub tcp_peer_address:
        unsafe extern "C" fn(socket: *const Tcp, address: *mut NetworkAddress) -> Status,
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
    pub heap_allocate:
        unsafe extern "C" fn(heap: *mut Heap, size: usize, allocation: *mut *mut c_void) -> Status,
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
    pub time_resolution: unsafe extern "C" fn(clock: u32, nanoseconds: *mut u64) -> Status,
    /// Suspend for at least the given nanoseconds on the executor's clock. A zero wait
    /// yields once inside a coroutine and returns at once outside one.
    pub time_sleep: unsafe extern "C" fn(nanoseconds: u64) -> Status,
    /// Fill a buffer from the host-granted entropy source. Zero size succeeds. The
    /// buffer contents are unspecified after a failure.
    pub random_fill: unsafe extern "C" fn(buffer: *mut c_void, size: usize) -> Status,
    /// Read the process's identifier.
    pub process_id: unsafe extern "C" fn(process: *const Process, id: *mut u32) -> Status,
    /// Read the identifier of the process that spawned this one, or zero (the root
    /// task) once that process has exited.
    pub process_parent_id: unsafe extern "C" fn(process: *const Process, id: *mut u32) -> Status,
    /// End the process with an exit status. Does not return on success; outside a
    /// native process coroutine it returns Unsupported.
    pub process_exit: unsafe extern "C" fn(process: *const Process, status: i32) -> Status,
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
    pub thread_join: unsafe extern "C" fn(thread: usize, result: *mut *mut c_void) -> Status,
    pub thread_detach: unsafe extern "C" fn(thread: usize) -> Status,
    pub thread_yield: unsafe extern "C" fn() -> Status,
    pub thread_current: unsafe extern "C" fn(thread: *mut usize) -> Status,
    pub thread_key_create: unsafe extern "C" fn(key: *mut usize) -> Status,
    pub thread_key_delete: unsafe extern "C" fn(key: usize) -> Status,
    pub thread_tls_get: unsafe extern "C" fn(key: usize, value: *mut *mut c_void) -> Status,
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
    pub condition_notify: unsafe extern "C" fn(condition: *mut Condition, all: bool) -> Status,
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
    pub file_duplicate: unsafe extern "C" fn(file: *const File, output: *mut *mut File) -> Status,
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
    pub file_is_terminal: unsafe extern "C" fn(file: *mut File, terminal: *mut bool) -> Status,
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
    pub process_child_id: unsafe extern "C" fn(child: *const Child, id: *mut u32) -> Status,
}

include!(concat!(env!("OUT_DIR"), "/api_layout.rs"));

#[cfg(not(feature = "network"))]
include!(concat!(env!("OUT_DIR"), "/disabled_network.rs"));

#[cfg(feature = "graphics")]
use crate::xila_graphics_call;
#[cfg(not(feature = "graphics"))]
include!(concat!(env!("OUT_DIR"), "/disabled_graphics.rs"));

/// Shared API table. Native code receives its address through XilaStartup.
pub static XILA_API: XilaApi = XilaApi {
    version: (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR,
    size: core::mem::size_of::<XilaApi>() as u32,
    directory_open: xila_directory_open,
    directory_close: xila_directory_close,
    directory_create: xila_directory_create,
    directory_remove: xila_directory_remove,
    directory_rename: xila_directory_rename,
    file_open: xila_file_open,
    file_read: xila_file_read,
    file_write: xila_file_write,
    file_seek: xila_file_seek,
    file_flush: xila_file_flush,
    file_close: xila_file_close,
    file_set_size: xila_file_set_size,
    file_get_statistics: xila_file_get_statistics,
    directory_get_statistics: xila_directory_get_statistics,
    directory_iterator_open: xila_directory_iterator_open,
    directory_iterator_next: xila_directory_iterator_next,
    directory_iterator_rewind: xila_directory_iterator_rewind,
    directory_iterator_close: xila_directory_iterator_close,
    time_monotonic: xila_time_monotonic,
    time_realtime: xila_time_realtime,
    network_close: xila_network_close,
    tcp_connect: xila_tcp_connect,
    tcp_listen: xila_tcp_listen,
    tcp_accept: xila_tcp_accept,
    tcp_close: xila_tcp_close,
    tcp_listener_close: xila_tcp_listener_close,
    tcp_shutdown: xila_tcp_shutdown,
    tcp_send: xila_tcp_send,
    tcp_receive: xila_tcp_receive,
    tcp_local_address: xila_tcp_local_address,
    tcp_peer_address: xila_tcp_peer_address,
    udp_bind: xila_udp_bind,
    udp_close: xila_udp_close,
    udp_send_to: xila_udp_send_to,
    udp_receive_from: xila_udp_receive_from,
    network_resolve: xila_network_resolve,
    heap_allocate: xila_heap_allocate,
    heap_reallocate: xila_heap_reallocate,
    heap_free: xila_heap_free,
    time_resolution: xila_time_resolution,
    time_sleep: xila_time_sleep,
    random_fill: xila_random_fill,
    process_id: xila_process_id,
    process_parent_id: xila_process_parent_id,
    process_exit: xila_process_exit,
    process_spawn: xila_process_spawn,
    process_wait: xila_process_wait,
    process_child_close: xila_process_child_close,
    env_get: xila_env_get,
    env_set: xila_env_set,
    env_remove: xila_env_remove,
    env_entry: xila_env_entry,
    thread_spawn: xila_thread_spawn,
    thread_join: xila_thread_join,
    thread_detach: xila_thread_detach,
    thread_yield: xila_thread_yield,
    thread_current: xila_thread_current,
    thread_key_create: xila_thread_key_create,
    thread_key_delete: xila_thread_key_delete,
    thread_tls_get: xila_thread_tls_get,
    thread_tls_set: xila_thread_tls_set,
    thread_libc: xila_thread_libc,
    mutex_init: xila_mutex_init,
    mutex_destroy: xila_mutex_destroy,
    mutex_lock: xila_mutex_lock,
    mutex_unlock: xila_mutex_unlock,
    condition_init: xila_condition_init,
    condition_destroy: xila_condition_destroy,
    condition_wait: xila_condition_wait,
    condition_notify: xila_condition_notify,
    thread_has_workers: xila_thread_has_workers,
    thread_stop_others: xila_thread_stop_others,
    thread_exit: xila_thread_exit,
    poll_wait: xila_poll_wait,
    file_read_at: xila_file_read_at,
    file_write_at: xila_file_write_at,
    file_duplicate: xila_file_duplicate,
    file_control: xila_file_control,
    file_is_terminal: xila_file_is_terminal,
    pipe_create: xila_pipe_create,
    graphics_call: xila_graphics_call,
    process_child_id: xila_process_child_id,
};
