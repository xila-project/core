/* Generated from XilaApi. Do not edit. */
#ifndef XILA_SDK_STUBS_H
#define XILA_SDK_STUBS_H

#include "application.h"

#ifdef __cplusplus
extern "C" {
#endif

int32_t _xila_start(const XilaStartup *startup);

XilaStatus xila_directory_open(XilaDirectory const * parent, char const * path, size_t path_length, uint32_t rights, XilaDirectory * * directory);
XilaStatus xila_directory_close(XilaDirectory * directory);
XilaStatus xila_directory_create(XilaDirectory const * directory, char const * path, size_t length);
XilaStatus xila_directory_remove(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags);
XilaStatus xila_directory_rename(XilaDirectory const * from, char const * from_path, size_t from_length, XilaDirectory const * to, char const * to_path, size_t to_length);
XilaStatus xila_file_open(XilaDirectory const * directory, char const * path, size_t path_length, uint32_t flags, XilaFile * * file);
XilaStatus xila_file_read(XilaFile * file, void * buffer, size_t size, size_t * count);
XilaStatus xila_file_write(XilaFile * file, void const * buffer, size_t size, size_t * count);
XilaStatus xila_file_seek(XilaFile * file, int64_t offset, uint32_t whence, uint64_t * position);
XilaStatus xila_file_flush(XilaFile * file);
XilaStatus xila_file_close(XilaFile * file);
XilaStatus xila_file_set_size(XilaFile * file, uint64_t size);
XilaStatus xila_file_get_statistics(XilaFile * file, XilaStatistics * output);
XilaStatus xila_directory_get_statistics(XilaDirectory const * directory, char const * path, size_t length, uint32_t flags, XilaStatistics * output);
XilaStatus xila_directory_iterator_open(XilaDirectory const * directory, char const * path, size_t length, XilaDirectoryIterator * * output);
XilaStatus xila_directory_iterator_next(XilaDirectoryIterator * iterator, char * name_buffer, size_t capacity, size_t * name_length, uint32_t * kind);
XilaStatus xila_directory_iterator_rewind(XilaDirectoryIterator * iterator);
XilaStatus xila_directory_iterator_close(XilaDirectoryIterator * iterator);
XilaStatus xila_time_monotonic(uint64_t * nanoseconds);
XilaStatus xila_time_realtime(int64_t * nanoseconds);
XilaStatus xila_network_close(XilaNetwork * network);
XilaStatus xila_tcp_connect(XilaNetwork const * network, XilaNetworkAddress const * address, uint32_t timeout_ms, XilaTcp * * connection);
XilaStatus xila_tcp_listen(XilaNetwork const * network, XilaNetworkAddress const * address, XilaTcpListener * * listener);
XilaStatus xila_tcp_accept(XilaTcpListener * listener, XilaTcp * * connection);
XilaStatus xila_tcp_close(XilaTcp * socket);
XilaStatus xila_tcp_listener_close(XilaTcpListener * socket);
XilaStatus xila_tcp_shutdown(XilaTcp * socket, uint32_t direction);
XilaStatus xila_tcp_send(XilaTcp * socket, void const * buffer, size_t size, size_t * count);
XilaStatus xila_tcp_receive(XilaTcp * socket, void * buffer, size_t size, size_t * count);
XilaStatus xila_tcp_local_address(XilaTcp const * socket, XilaNetworkAddress * address);
XilaStatus xila_tcp_peer_address(XilaTcp const * socket, XilaNetworkAddress * address);
XilaStatus xila_udp_bind(XilaNetwork const * network, XilaNetworkAddress const * address, XilaUdp * * socket);
XilaStatus xila_udp_close(XilaUdp * socket);
XilaStatus xila_udp_send_to(XilaUdp * socket, void const * buffer, size_t size, XilaNetworkAddress const * address, size_t * count);
XilaStatus xila_udp_receive_from(XilaUdp * socket, void * buffer, size_t size, XilaNetworkAddress * address, size_t * count);
XilaStatus xila_network_resolve(XilaNetwork const * network, char const * host, size_t host_length, uint32_t family, XilaNetworkAddress * addresses, size_t * count);
XilaStatus xila_heap_allocate(XilaHeap * heap, size_t size, void * * allocation);
XilaStatus xila_heap_reallocate(XilaHeap * heap, void * pointer, size_t size, void * * allocation);
XilaStatus xila_heap_free(XilaHeap * heap, void * pointer);

#ifdef __cplusplus
}
#endif

#endif
