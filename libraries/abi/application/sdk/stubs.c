/* Generated from XilaApi. Do not edit. */
#include "stubs.h"

extern const XilaApi *xila_sdk_api;

static XilaStatus unavailable_directory_open(XilaDirectory const * parent, char const * path, size_t path_length, uint32_t rights, XilaDirectory * * directory) {
    (void)parent;
    (void)path;
    (void)path_length;
    (void)rights;
    (void)directory;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_open(XilaDirectory const * parent, char const * path, size_t path_length, uint32_t rights, XilaDirectory * * directory) {
    return xila_sdk_api->directory_open(parent, path, path_length, rights, directory);
}

static XilaStatus unavailable_directory_close(XilaDirectory * directory) {
    (void)directory;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_close(XilaDirectory * directory) {
    return xila_sdk_api->directory_close(directory);
}

static XilaStatus unavailable_directory_create(XilaDirectory const * directory, char const * path, size_t length) {
    (void)directory;
    (void)path;
    (void)length;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_create(XilaDirectory const * directory, char const * path, size_t length) {
    return xila_sdk_api->directory_create(directory, path, length);
}

static XilaStatus unavailable_directory_remove(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags) {
    (void)directory;
    (void)path;
    (void)length;
    (void)flags;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_remove(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags) {
    return xila_sdk_api->directory_remove(directory, path, length, flags);
}

static XilaStatus unavailable_directory_rename(XilaDirectory const * from, char const * from_path, size_t from_length, XilaDirectory const * to, char const * to_path, size_t to_length) {
    (void)from;
    (void)from_path;
    (void)from_length;
    (void)to;
    (void)to_path;
    (void)to_length;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_rename(XilaDirectory const * from, char const * from_path, size_t from_length, XilaDirectory const * to, char const * to_path, size_t to_length) {
    return xila_sdk_api->directory_rename(from, from_path, from_length, to, to_path, to_length);
}

static XilaStatus unavailable_file_open(XilaDirectory const * directory, char const * path, size_t path_length, uint32_t flags, XilaFile * * file) {
    (void)directory;
    (void)path;
    (void)path_length;
    (void)flags;
    (void)file;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_open(XilaDirectory const * directory, char const * path, size_t path_length, uint32_t flags, XilaFile * * file) {
    return xila_sdk_api->file_open(directory, path, path_length, flags, file);
}

static XilaStatus unavailable_file_read(XilaFile * file, void * buffer, size_t size, size_t * count) {
    (void)file;
    (void)buffer;
    (void)size;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_read(XilaFile * file, void * buffer, size_t size, size_t * count) {
    return xila_sdk_api->file_read(file, buffer, size, count);
}

static XilaStatus unavailable_file_write(XilaFile * file, void const * buffer, size_t size, size_t * count) {
    (void)file;
    (void)buffer;
    (void)size;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_write(XilaFile * file, void const * buffer, size_t size, size_t * count) {
    return xila_sdk_api->file_write(file, buffer, size, count);
}

static XilaStatus unavailable_file_seek(XilaFile * file, int64_t offset, uint32_t whence, uint64_t * position) {
    (void)file;
    (void)offset;
    (void)whence;
    (void)position;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_seek(XilaFile * file, int64_t offset, uint32_t whence, uint64_t * position) {
    return xila_sdk_api->file_seek(file, offset, whence, position);
}

static XilaStatus unavailable_file_flush(XilaFile * file) {
    (void)file;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_flush(XilaFile * file) {
    return xila_sdk_api->file_flush(file);
}

static XilaStatus unavailable_file_close(XilaFile * file) {
    (void)file;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_close(XilaFile * file) {
    return xila_sdk_api->file_close(file);
}

static XilaStatus unavailable_file_set_size(XilaFile * file, uint64_t size) {
    (void)file;
    (void)size;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_set_size(XilaFile * file, uint64_t size) {
    return xila_sdk_api->file_set_size(file, size);
}

static XilaStatus unavailable_file_get_statistics(XilaFile * file, XilaStatistics * output) {
    (void)file;
    (void)output;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_file_get_statistics(XilaFile * file, XilaStatistics * output) {
    return xila_sdk_api->file_get_statistics(file, output);
}

static XilaStatus unavailable_directory_get_statistics(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags, XilaStatistics * output) {
    (void)directory;
    (void)path;
    (void)length;
    (void)flags;
    (void)output;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_get_statistics(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags, XilaStatistics * output) {
    return xila_sdk_api->directory_get_statistics(directory, path, length, flags, output);
}

static XilaStatus unavailable_directory_iterator_open(XilaDirectory const * directory, char const * path, size_t length, XilaDirectoryIterator * * output) {
    (void)directory;
    (void)path;
    (void)length;
    (void)output;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_iterator_open(XilaDirectory const * directory, char const * path, size_t length, XilaDirectoryIterator * * output) {
    return xila_sdk_api->directory_iterator_open(directory, path, length, output);
}

static XilaStatus unavailable_directory_iterator_next(XilaDirectoryIterator * iterator, char * name_buffer, size_t capacity, size_t * name_length, uint32_t * kind) {
    (void)iterator;
    (void)name_buffer;
    (void)capacity;
    (void)name_length;
    (void)kind;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_iterator_next(XilaDirectoryIterator * iterator, char * name_buffer, size_t capacity, size_t * name_length, uint32_t * kind) {
    return xila_sdk_api->directory_iterator_next(iterator, name_buffer, capacity, name_length, kind);
}

static XilaStatus unavailable_directory_iterator_rewind(XilaDirectoryIterator * iterator) {
    (void)iterator;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_iterator_rewind(XilaDirectoryIterator * iterator) {
    return xila_sdk_api->directory_iterator_rewind(iterator);
}

static XilaStatus unavailable_directory_iterator_close(XilaDirectoryIterator * iterator) {
    (void)iterator;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_directory_iterator_close(XilaDirectoryIterator * iterator) {
    return xila_sdk_api->directory_iterator_close(iterator);
}

static XilaStatus unavailable_time_monotonic(uint64_t * nanoseconds) {
    (void)nanoseconds;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_time_monotonic(uint64_t * nanoseconds) {
    return xila_sdk_api->time_monotonic(nanoseconds);
}

static XilaStatus unavailable_time_realtime(int64_t * nanoseconds) {
    (void)nanoseconds;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_time_realtime(int64_t * nanoseconds) {
    return xila_sdk_api->time_realtime(nanoseconds);
}

static XilaStatus unavailable_network_close(XilaNetwork * network) {
    (void)network;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_network_close(XilaNetwork * network) {
    return xila_sdk_api->network_close(network);
}

static XilaStatus unavailable_tcp_connect(XilaNetwork const * network, XilaNetworkAddress const * address, uint32_t timeout_ms, XilaTcp * * connection) {
    (void)network;
    (void)address;
    (void)timeout_ms;
    (void)connection;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_connect(XilaNetwork const * network, XilaNetworkAddress const * address, uint32_t timeout_ms, XilaTcp * * connection) {
    return xila_sdk_api->tcp_connect(network, address, timeout_ms, connection);
}

static XilaStatus unavailable_tcp_listen(XilaNetwork const * network, XilaNetworkAddress const * address, XilaTcpListener * * listener) {
    (void)network;
    (void)address;
    (void)listener;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_listen(XilaNetwork const * network, XilaNetworkAddress const * address, XilaTcpListener * * listener) {
    return xila_sdk_api->tcp_listen(network, address, listener);
}

static XilaStatus unavailable_tcp_accept(XilaTcpListener * listener, XilaTcp * * connection) {
    (void)listener;
    (void)connection;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_accept(XilaTcpListener * listener, XilaTcp * * connection) {
    return xila_sdk_api->tcp_accept(listener, connection);
}

static XilaStatus unavailable_tcp_close(XilaTcp * socket) {
    (void)socket;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_close(XilaTcp * socket) {
    return xila_sdk_api->tcp_close(socket);
}

static XilaStatus unavailable_tcp_listener_close(XilaTcpListener * socket) {
    (void)socket;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_listener_close(XilaTcpListener * socket) {
    return xila_sdk_api->tcp_listener_close(socket);
}

static XilaStatus unavailable_tcp_shutdown(XilaTcp * socket, uint32_t direction) {
    (void)socket;
    (void)direction;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_shutdown(XilaTcp * socket, uint32_t direction) {
    return xila_sdk_api->tcp_shutdown(socket, direction);
}

static XilaStatus unavailable_tcp_send(XilaTcp * socket, void const * buffer, size_t size, size_t * count) {
    (void)socket;
    (void)buffer;
    (void)size;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_send(XilaTcp * socket, void const * buffer, size_t size, size_t * count) {
    return xila_sdk_api->tcp_send(socket, buffer, size, count);
}

static XilaStatus unavailable_tcp_receive(XilaTcp * socket, void * buffer, size_t size, size_t * count) {
    (void)socket;
    (void)buffer;
    (void)size;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_receive(XilaTcp * socket, void * buffer, size_t size, size_t * count) {
    return xila_sdk_api->tcp_receive(socket, buffer, size, count);
}

static XilaStatus unavailable_tcp_local_address(XilaTcp const * socket, XilaNetworkAddress * address) {
    (void)socket;
    (void)address;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_local_address(XilaTcp const * socket, XilaNetworkAddress * address) {
    return xila_sdk_api->tcp_local_address(socket, address);
}

static XilaStatus unavailable_tcp_peer_address(XilaTcp const * socket, XilaNetworkAddress * address) {
    (void)socket;
    (void)address;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_tcp_peer_address(XilaTcp const * socket, XilaNetworkAddress * address) {
    return xila_sdk_api->tcp_peer_address(socket, address);
}

static XilaStatus unavailable_udp_bind(XilaNetwork const * network, XilaNetworkAddress const * address, XilaUdp * * socket) {
    (void)network;
    (void)address;
    (void)socket;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_udp_bind(XilaNetwork const * network, XilaNetworkAddress const * address, XilaUdp * * socket) {
    return xila_sdk_api->udp_bind(network, address, socket);
}

static XilaStatus unavailable_udp_close(XilaUdp * socket) {
    (void)socket;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_udp_close(XilaUdp * socket) {
    return xila_sdk_api->udp_close(socket);
}

static XilaStatus unavailable_udp_send_to(XilaUdp * socket, void const * buffer, size_t size, XilaNetworkAddress const * address, size_t * count) {
    (void)socket;
    (void)buffer;
    (void)size;
    (void)address;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_udp_send_to(XilaUdp * socket, void const * buffer, size_t size, XilaNetworkAddress const * address, size_t * count) {
    return xila_sdk_api->udp_send_to(socket, buffer, size, address, count);
}

static XilaStatus unavailable_udp_receive_from(XilaUdp * socket, void * buffer, size_t size, XilaNetworkAddress * address, size_t * count) {
    (void)socket;
    (void)buffer;
    (void)size;
    (void)address;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_udp_receive_from(XilaUdp * socket, void * buffer, size_t size, XilaNetworkAddress * address, size_t * count) {
    return xila_sdk_api->udp_receive_from(socket, buffer, size, address, count);
}

static XilaStatus unavailable_network_resolve(XilaNetwork const * network, char const * host, size_t host_length, uint32_t family, XilaNetworkAddress * addresses, size_t * count) {
    (void)network;
    (void)host;
    (void)host_length;
    (void)family;
    (void)addresses;
    (void)count;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_network_resolve(XilaNetwork const * network, char const * host, size_t host_length, uint32_t family, XilaNetworkAddress * addresses, size_t * count) {
    return xila_sdk_api->network_resolve(network, host, host_length, family, addresses, count);
}

static XilaStatus unavailable_heap_allocate(XilaHeap * heap, size_t size, void * * allocation) {
    (void)heap;
    (void)size;
    (void)allocation;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_heap_allocate(XilaHeap * heap, size_t size, void * * allocation) {
    return xila_sdk_api->heap_allocate(heap, size, allocation);
}

static XilaStatus unavailable_heap_reallocate(XilaHeap * heap, void * pointer, size_t size, void * * allocation) {
    (void)heap;
    (void)pointer;
    (void)size;
    (void)allocation;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_heap_reallocate(XilaHeap * heap, void * pointer, size_t size, void * * allocation) {
    return xila_sdk_api->heap_reallocate(heap, pointer, size, allocation);
}

static XilaStatus unavailable_heap_free(XilaHeap * heap, void * pointer) {
    (void)heap;
    (void)pointer;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_heap_free(XilaHeap * heap, void * pointer) {
    return xila_sdk_api->heap_free(heap, pointer);
}

static XilaStatus unavailable_time_resolution(uint32_t clock, uint64_t * nanoseconds) {
    (void)clock;
    (void)nanoseconds;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_time_resolution(uint32_t clock, uint64_t * nanoseconds) {
    return xila_sdk_api->time_resolution(clock, nanoseconds);
}

static XilaStatus unavailable_time_sleep(uint64_t nanoseconds) {
    (void)nanoseconds;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_time_sleep(uint64_t nanoseconds) {
    return xila_sdk_api->time_sleep(nanoseconds);
}

static XilaStatus unavailable_random_fill(void * buffer, size_t size) {
    (void)buffer;
    (void)size;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_random_fill(void * buffer, size_t size) {
    return xila_sdk_api->random_fill(buffer, size);
}

const XilaApi xila_sdk_fallback_api = {
    .version = (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR,
    .size = sizeof(XilaApi),
    .directory_open = unavailable_directory_open,
    .directory_close = unavailable_directory_close,
    .directory_create = unavailable_directory_create,
    .directory_remove = unavailable_directory_remove,
    .directory_rename = unavailable_directory_rename,
    .file_open = unavailable_file_open,
    .file_read = unavailable_file_read,
    .file_write = unavailable_file_write,
    .file_seek = unavailable_file_seek,
    .file_flush = unavailable_file_flush,
    .file_close = unavailable_file_close,
    .file_set_size = unavailable_file_set_size,
    .file_get_statistics = unavailable_file_get_statistics,
    .directory_get_statistics = unavailable_directory_get_statistics,
    .directory_iterator_open = unavailable_directory_iterator_open,
    .directory_iterator_next = unavailable_directory_iterator_next,
    .directory_iterator_rewind = unavailable_directory_iterator_rewind,
    .directory_iterator_close = unavailable_directory_iterator_close,
    .time_monotonic = unavailable_time_monotonic,
    .time_realtime = unavailable_time_realtime,
    .network_close = unavailable_network_close,
    .tcp_connect = unavailable_tcp_connect,
    .tcp_listen = unavailable_tcp_listen,
    .tcp_accept = unavailable_tcp_accept,
    .tcp_close = unavailable_tcp_close,
    .tcp_listener_close = unavailable_tcp_listener_close,
    .tcp_shutdown = unavailable_tcp_shutdown,
    .tcp_send = unavailable_tcp_send,
    .tcp_receive = unavailable_tcp_receive,
    .tcp_local_address = unavailable_tcp_local_address,
    .tcp_peer_address = unavailable_tcp_peer_address,
    .udp_bind = unavailable_udp_bind,
    .udp_close = unavailable_udp_close,
    .udp_send_to = unavailable_udp_send_to,
    .udp_receive_from = unavailable_udp_receive_from,
    .network_resolve = unavailable_network_resolve,
    .heap_allocate = unavailable_heap_allocate,
    .heap_reallocate = unavailable_heap_reallocate,
    .heap_free = unavailable_heap_free,
    .time_resolution = unavailable_time_resolution,
    .time_sleep = unavailable_time_sleep,
    .random_fill = unavailable_random_fill,
};
