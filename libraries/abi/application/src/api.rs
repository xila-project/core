use crate::*;
use core::ffi::{c_char, c_void};

/// Native application entry points, in slot order. The header precedes slot zero.
/// Networking slots return Unsupported when the runtime omits networking.
#[repr(C)]
pub struct XilaApi {
    /// Packed major and minor ABI version.
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
}

include!(concat!(env!("OUT_DIR"), "/api_layout.rs"));

#[cfg(not(feature = "network"))]
include!(concat!(env!("OUT_DIR"), "/disabled_network.rs"));

/// Shared API table. Native code receives its address through XilaStartup.
pub static XILA_API: XilaApi = XilaApi {
    version: (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR,
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
};
