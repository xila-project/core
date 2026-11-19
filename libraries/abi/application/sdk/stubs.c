/* Generated from XilaApi. Do not edit. */
#include "stubs.h"

extern const XilaApi *xila_sdk_api;

XilaStatus xila_directory_open(XilaDirectory const * parent, char const * path, size_t path_length, uint32_t rights, XilaDirectory * * directory) {
    return xila_sdk_api->directory_open(parent, path, path_length, rights, directory);
}

XilaStatus xila_directory_close(XilaDirectory * directory) {
    return xila_sdk_api->directory_close(directory);
}

XilaStatus xila_directory_create(XilaDirectory const * directory, char const * path, size_t length) {
    return xila_sdk_api->directory_create(directory, path, length);
}

XilaStatus xila_directory_remove(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags) {
    return xila_sdk_api->directory_remove(directory, path, length, flags);
}

XilaStatus xila_directory_rename(XilaDirectory const * from, char const * from_path, size_t from_length, XilaDirectory const * to, char const * to_path, size_t to_length) {
    return xila_sdk_api->directory_rename(from, from_path, from_length, to, to_path, to_length);
}

XilaStatus xila_file_open(XilaDirectory const * directory, char const * path, size_t path_length, uint32_t flags, XilaFile * * file) {
    return xila_sdk_api->file_open(directory, path, path_length, flags, file);
}

XilaStatus xila_file_read(XilaFile * file, void * buffer, size_t size, size_t * count) {
    return xila_sdk_api->file_read(file, buffer, size, count);
}

XilaStatus xila_file_write(XilaFile * file, void const * buffer, size_t size, size_t * count) {
    return xila_sdk_api->file_write(file, buffer, size, count);
}

XilaStatus xila_file_seek(XilaFile * file, int64_t offset, uint32_t whence, uint64_t * position) {
    return xila_sdk_api->file_seek(file, offset, whence, position);
}

XilaStatus xila_file_flush(XilaFile * file) {
    return xila_sdk_api->file_flush(file);
}

XilaStatus xila_file_close(XilaFile * file) {
    return xila_sdk_api->file_close(file);
}

XilaStatus xila_file_set_size(XilaFile * file, uint64_t size) {
    return xila_sdk_api->file_set_size(file, size);
}

XilaStatus xila_file_get_statistics(XilaFile * file, XilaStatistics * output) {
    return xila_sdk_api->file_get_statistics(file, output);
}

XilaStatus xila_directory_get_statistics(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags, XilaStatistics * output) {
    return xila_sdk_api->directory_get_statistics(directory, path, length, flags, output);
}

XilaStatus xila_directory_iterator_open(XilaDirectory const * directory, char const * path, size_t length, XilaDirectoryIterator * * output) {
    return xila_sdk_api->directory_iterator_open(directory, path, length, output);
}

XilaStatus xila_directory_iterator_next(XilaDirectoryIterator * iterator, char * name_buffer, size_t capacity, size_t * name_length, uint32_t * kind) {
    return xila_sdk_api->directory_iterator_next(iterator, name_buffer, capacity, name_length, kind);
}

XilaStatus xila_directory_iterator_rewind(XilaDirectoryIterator * iterator) {
    return xila_sdk_api->directory_iterator_rewind(iterator);
}

XilaStatus xila_directory_iterator_close(XilaDirectoryIterator * iterator) {
    return xila_sdk_api->directory_iterator_close(iterator);
}

XilaStatus xila_time_monotonic(uint64_t * nanoseconds) {
    return xila_sdk_api->time_monotonic(nanoseconds);
}

XilaStatus xila_time_realtime(int64_t * nanoseconds) {
    return xila_sdk_api->time_realtime(nanoseconds);
}

XilaStatus xila_network_close(XilaNetwork * network) {
    return xila_sdk_api->network_close(network);
}

XilaStatus xila_tcp_connect(XilaNetwork const * network, XilaNetworkAddress const * address, uint32_t timeout_ms, XilaTcp * * connection) {
    return xila_sdk_api->tcp_connect(network, address, timeout_ms, connection);
}

XilaStatus xila_tcp_listen(XilaNetwork const * network, XilaNetworkAddress const * address, XilaTcpListener * * listener) {
    return xila_sdk_api->tcp_listen(network, address, listener);
}

XilaStatus xila_tcp_accept(XilaTcpListener * listener, XilaTcp * * connection) {
    return xila_sdk_api->tcp_accept(listener, connection);
}

XilaStatus xila_tcp_close(XilaTcp * socket) {
    return xila_sdk_api->tcp_close(socket);
}

XilaStatus xila_tcp_listener_close(XilaTcpListener * socket) {
    return xila_sdk_api->tcp_listener_close(socket);
}

XilaStatus xila_tcp_shutdown(XilaTcp * socket, uint32_t direction) {
    return xila_sdk_api->tcp_shutdown(socket, direction);
}

XilaStatus xila_tcp_send(XilaTcp * socket, void const * buffer, size_t size, size_t * count) {
    return xila_sdk_api->tcp_send(socket, buffer, size, count);
}

XilaStatus xila_tcp_receive(XilaTcp * socket, void * buffer, size_t size, size_t * count) {
    return xila_sdk_api->tcp_receive(socket, buffer, size, count);
}

XilaStatus xila_tcp_local_address(XilaTcp const * socket, XilaNetworkAddress * address) {
    return xila_sdk_api->tcp_local_address(socket, address);
}

XilaStatus xila_tcp_peer_address(XilaTcp const * socket, XilaNetworkAddress * address) {
    return xila_sdk_api->tcp_peer_address(socket, address);
}

XilaStatus xila_udp_bind(XilaNetwork const * network, XilaNetworkAddress const * address, XilaUdp * * socket) {
    return xila_sdk_api->udp_bind(network, address, socket);
}

XilaStatus xila_udp_close(XilaUdp * socket) {
    return xila_sdk_api->udp_close(socket);
}

XilaStatus xila_udp_send_to(XilaUdp * socket, void const * buffer, size_t size, XilaNetworkAddress const * address, size_t * count) {
    return xila_sdk_api->udp_send_to(socket, buffer, size, address, count);
}

XilaStatus xila_udp_receive_from(XilaUdp * socket, void * buffer, size_t size, XilaNetworkAddress * address, size_t * count) {
    return xila_sdk_api->udp_receive_from(socket, buffer, size, address, count);
}

XilaStatus xila_network_resolve(XilaNetwork const * network, char const * host, size_t host_length, uint32_t family, XilaNetworkAddress * addresses, size_t * count) {
    return xila_sdk_api->network_resolve(network, host, host_length, family, addresses, count);
}

XilaStatus xila_heap_allocate(XilaHeap * heap, size_t size, void * * allocation) {
    return xila_sdk_api->heap_allocate(heap, size, allocation);
}

XilaStatus xila_heap_reallocate(XilaHeap * heap, void * pointer, size_t size, void * * allocation) {
    return xila_sdk_api->heap_reallocate(heap, pointer, size, allocation);
}

XilaStatus xila_heap_free(XilaHeap * heap, void * pointer) {
    return xila_sdk_api->heap_free(heap, pointer);
}
