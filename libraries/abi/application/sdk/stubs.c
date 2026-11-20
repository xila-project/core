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

static XilaStatus unavailable_process_id(XilaProcess const * process, uint32_t * id) {
    (void)process;
    (void)id;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_process_id(XilaProcess const * process, uint32_t * id) {
    return xila_sdk_api->process_id(process, id);
}

static XilaStatus unavailable_process_parent_id(XilaProcess const * process, uint32_t * id) {
    (void)process;
    (void)id;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_process_parent_id(XilaProcess const * process, uint32_t * id) {
    return xila_sdk_api->process_parent_id(process, id);
}

static XilaStatus unavailable_process_exit(XilaProcess const * process, int32_t status) {
    (void)process;
    (void)status;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_process_exit(XilaProcess const * process, int32_t status) {
    return xila_sdk_api->process_exit(process, status);
}

static XilaStatus unavailable_process_spawn(XilaProcess const * process, XilaDirectory const * directory, char const * path, size_t path_length, XilaSpawn const * spawn, XilaChild * * child) {
    (void)process;
    (void)directory;
    (void)path;
    (void)path_length;
    (void)spawn;
    (void)child;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_process_spawn(XilaProcess const * process, XilaDirectory const * directory, char const * path, size_t path_length, XilaSpawn const * spawn, XilaChild * * child) {
    return xila_sdk_api->process_spawn(process, directory, path, path_length, spawn, child);
}

static XilaStatus unavailable_process_wait(XilaProcess const * process, XilaChild * child, uint32_t flags, int32_t * status) {
    (void)process;
    (void)child;
    (void)flags;
    (void)status;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_process_wait(XilaProcess const * process, XilaChild * child, uint32_t flags, int32_t * status) {
    return xila_sdk_api->process_wait(process, child, flags, status);
}

static XilaStatus unavailable_process_child_close(XilaChild * child) {
    (void)child;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_process_child_close(XilaChild * child) {
    return xila_sdk_api->process_child_close(child);
}

static XilaStatus unavailable_env_get(XilaProcess const * process, char const * name, size_t name_length, char * buffer, size_t capacity, size_t * value_length) {
    (void)process;
    (void)name;
    (void)name_length;
    (void)buffer;
    (void)capacity;
    (void)value_length;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_env_get(XilaProcess const * process, char const * name, size_t name_length, char * buffer, size_t capacity, size_t * value_length) {
    return xila_sdk_api->env_get(process, name, name_length, buffer, capacity, value_length);
}

static XilaStatus unavailable_env_set(XilaProcess const * process, char const * name, size_t name_length, char const * value, size_t value_length) {
    (void)process;
    (void)name;
    (void)name_length;
    (void)value;
    (void)value_length;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_env_set(XilaProcess const * process, char const * name, size_t name_length, char const * value, size_t value_length) {
    return xila_sdk_api->env_set(process, name, name_length, value, value_length);
}

static XilaStatus unavailable_env_remove(XilaProcess const * process, char const * name, size_t name_length) {
    (void)process;
    (void)name;
    (void)name_length;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_env_remove(XilaProcess const * process, char const * name, size_t name_length) {
    return xila_sdk_api->env_remove(process, name, name_length);
}

static XilaStatus unavailable_thread_spawn(XilaProcess const * process, size_t stack_size, XilaThreadEntry entry, void * argument, size_t * thread) {
    (void)process;
    (void)stack_size;
    (void)entry;
    (void)argument;
    (void)thread;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_spawn(XilaProcess const * process, size_t stack_size, XilaThreadEntry entry, void * argument, size_t * thread) {
    return xila_sdk_api->thread_spawn(process, stack_size, entry, argument, thread);
}

static XilaStatus unavailable_thread_join(size_t thread, void * * result) {
    (void)thread;
    (void)result;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_join(size_t thread, void * * result) {
    return xila_sdk_api->thread_join(thread, result);
}

static XilaStatus unavailable_thread_detach(size_t thread) {
    (void)thread;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_detach(size_t thread) {
    return xila_sdk_api->thread_detach(thread);
}

static XilaStatus unavailable_thread_yield(void) {
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_yield(void) {
    return xila_sdk_api->thread_yield();
}

static XilaStatus unavailable_thread_current(size_t * thread) {
    (void)thread;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_current(size_t * thread) {
    return xila_sdk_api->thread_current(thread);
}

static XilaStatus unavailable_thread_key_create(size_t * key) {
    (void)key;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_key_create(size_t * key) {
    return xila_sdk_api->thread_key_create(key);
}

static XilaStatus unavailable_thread_key_delete(size_t key) {
    (void)key;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_key_delete(size_t key) {
    return xila_sdk_api->thread_key_delete(key);
}

static XilaStatus unavailable_thread_tls_get(size_t key, void * * value) {
    (void)key;
    (void)value;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_tls_get(size_t key, void * * value) {
    return xila_sdk_api->thread_tls_get(key, value);
}

static XilaStatus unavailable_thread_tls_set(size_t key, void * value) {
    (void)key;
    (void)value;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_tls_set(size_t key, void * value) {
    return xila_sdk_api->thread_tls_set(key, value);
}

static XilaStatus unavailable_thread_libc(XilaThreadLibc * * location) {
    (void)location;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_libc(XilaThreadLibc * * location) {
    return xila_sdk_api->thread_libc(location);
}

static XilaStatus unavailable_mutex_init(XilaMutex * mutex, uint32_t flags) {
    (void)mutex;
    (void)flags;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_mutex_init(XilaMutex * mutex, uint32_t flags) {
    return xila_sdk_api->mutex_init(mutex, flags);
}

static XilaStatus unavailable_mutex_destroy(XilaMutex * mutex) {
    (void)mutex;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_mutex_destroy(XilaMutex * mutex) {
    return xila_sdk_api->mutex_destroy(mutex);
}

static XilaStatus unavailable_mutex_lock(XilaMutex * mutex, uint32_t flags) {
    (void)mutex;
    (void)flags;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_mutex_lock(XilaMutex * mutex, uint32_t flags) {
    return xila_sdk_api->mutex_lock(mutex, flags);
}

static XilaStatus unavailable_mutex_unlock(XilaMutex * mutex) {
    (void)mutex;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_mutex_unlock(XilaMutex * mutex) {
    return xila_sdk_api->mutex_unlock(mutex);
}

static XilaStatus unavailable_condition_init(XilaCondition * condition) {
    (void)condition;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_condition_init(XilaCondition * condition) {
    return xila_sdk_api->condition_init(condition);
}

static XilaStatus unavailable_condition_destroy(XilaCondition * condition) {
    (void)condition;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_condition_destroy(XilaCondition * condition) {
    return xila_sdk_api->condition_destroy(condition);
}

static XilaStatus unavailable_condition_wait(XilaCondition * condition, XilaMutex * mutex, uint32_t clock, int64_t const * deadline) {
    (void)condition;
    (void)mutex;
    (void)clock;
    (void)deadline;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_condition_wait(XilaCondition * condition, XilaMutex * mutex, uint32_t clock, int64_t const * deadline) {
    return xila_sdk_api->condition_wait(condition, mutex, clock, deadline);
}

static XilaStatus unavailable_condition_notify(XilaCondition * condition, bool all) {
    (void)condition;
    (void)all;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_condition_notify(XilaCondition * condition, bool all) {
    return xila_sdk_api->condition_notify(condition, all);
}

static XilaStatus unavailable_thread_has_workers(bool * live) {
    (void)live;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_has_workers(bool * live) {
    return xila_sdk_api->thread_has_workers(live);
}

static XilaStatus unavailable_thread_stop_others(XilaProcess const * process) {
    (void)process;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_stop_others(XilaProcess const * process) {
    return xila_sdk_api->thread_stop_others(process);
}

static XilaStatus unavailable_thread_exit(void * result) {
    (void)result;
    return XILA_STATUS_UNAVAILABLE;
}

XilaStatus xila_thread_exit(void * result) {
    return xila_sdk_api->thread_exit(result);
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
    .process_id = unavailable_process_id,
    .process_parent_id = unavailable_process_parent_id,
    .process_exit = unavailable_process_exit,
    .process_spawn = unavailable_process_spawn,
    .process_wait = unavailable_process_wait,
    .process_child_close = unavailable_process_child_close,
    .env_get = unavailable_env_get,
    .env_set = unavailable_env_set,
    .env_remove = unavailable_env_remove,
    .thread_spawn = unavailable_thread_spawn,
    .thread_join = unavailable_thread_join,
    .thread_detach = unavailable_thread_detach,
    .thread_yield = unavailable_thread_yield,
    .thread_current = unavailable_thread_current,
    .thread_key_create = unavailable_thread_key_create,
    .thread_key_delete = unavailable_thread_key_delete,
    .thread_tls_get = unavailable_thread_tls_get,
    .thread_tls_set = unavailable_thread_tls_set,
    .thread_libc = unavailable_thread_libc,
    .mutex_init = unavailable_mutex_init,
    .mutex_destroy = unavailable_mutex_destroy,
    .mutex_lock = unavailable_mutex_lock,
    .mutex_unlock = unavailable_mutex_unlock,
    .condition_init = unavailable_condition_init,
    .condition_destroy = unavailable_condition_destroy,
    .condition_wait = unavailable_condition_wait,
    .condition_notify = unavailable_condition_notify,
    .thread_has_workers = unavailable_thread_has_workers,
    .thread_stop_others = unavailable_thread_stop_others,
    .thread_exit = unavailable_thread_exit,
};
