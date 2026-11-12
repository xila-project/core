#include "application.h"
#include <string.h>

int application_c_round_trip(const XilaDirectory *directory) {
    XilaFile *file = NULL;
    size_t count = 99;
    uint64_t position = 99;
    char buffer[5] = {0};
    if (xila_abi_version() != ((XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR)) return 1;
    if (xila_file_open(directory, "c-file", 6, XILA_OPEN_READ | XILA_OPEN_WRITE | XILA_OPEN_CREATE, &file) != XILA_STATUS_OK) return 2;
    if (xila_file_write(file, "hello", 5, &count) != XILA_STATUS_OK || count != 5) return 3;
    if (xila_file_seek(file, 0, XILA_SEEK_START, &position) != XILA_STATUS_OK || position != 0) return 4;
    if (xila_file_read(file, buffer, sizeof buffer, &count) != XILA_STATUS_OK || count != 5 || memcmp(buffer, "hello", 5) != 0) return 5;
    if (xila_file_flush(file) != XILA_STATUS_OK) return 6;
    return xila_file_close(file) == XILA_STATUS_OK ? 0 : 7;
}

#define CHECK(expression) do { if (!(expression)) return __LINE__; } while (0)

int application_c_file_system(const XilaDirectory *directory) {
    XilaFile *file = NULL;
    XilaDirectory *read_only = NULL;
    XilaDirectory *write_only = NULL;
    XilaDirectoryIterator *iterator = NULL;
    XilaStatistics stats = {.size = sizeof stats};
    struct { XilaStatistics stats; uint64_t tail; } extended = {{.size = sizeof extended}, UINT64_C(0x12345678)};
    size_t length = 99;
    uint32_t kind = 99;
    char names[32];
    uint64_t position = 99;
    CHECK(sizeof stats == 72);
    CHECK(xila_directory_create(directory, "entries", 7) == XILA_STATUS_OK);
    CHECK(xila_directory_create(directory, "entries", 7) == XILA_STATUS_ALREADY_EXISTS);
    CHECK(xila_directory_open(directory, "entries", 7, XILA_RIGHT_READ, &read_only) == XILA_STATUS_OK);
    CHECK(xila_directory_open(directory, "entries", 7, XILA_RIGHT_WRITE, &write_only) == XILA_STATUS_OK);
    CHECK(xila_directory_create(read_only, "bad", 3) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_directory_remove(read_only, "bad", 3, 0) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_directory_rename(read_only, "bad", 3, directory, "bad", 3) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_directory_rename(directory, "bad", 3, read_only, "bad", 3) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_directory_iterator_open(write_only, "", 0, &iterator) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_directory_get_statistics(write_only, "", 0, 0, &stats) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_directory_get_statistics(read_only, "", 0, 0, &stats) == XILA_STATUS_OK);
    CHECK(stats.kind == XILA_KIND_DIRECTORY);
    stats.size = 4;
    stats.kind = 99;
    CHECK(xila_directory_get_statistics(directory, "entries", 7, 0, &stats) == XILA_STATUS_INVALID_ARGUMENT && stats.kind == 99 && stats.size == 4);
    CHECK(xila_directory_get_statistics(directory, "entries", 7, XILA_STATISTICS_NO_FOLLOW, &extended.stats) == XILA_STATUS_OK);
    CHECK(extended.stats.size == sizeof stats && extended.tail == UINT64_C(0x12345678));
    CHECK(xila_directory_get_statistics(directory, "entries", 7, 2, &extended.stats) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(xila_file_open(directory, "entries/alpha", 13, XILA_OPEN_CREATE | XILA_OPEN_READ | XILA_OPEN_WRITE, &file) == XILA_STATUS_OK);
    CHECK(xila_file_write(file, "abcdef", 6, &length) == XILA_STATUS_OK);
    stats.size = sizeof stats;
    CHECK(xila_file_get_statistics(file, &stats) == XILA_STATUS_OK && stats.kind == XILA_KIND_FILE && stats.file_size == 6);
    stats.size = 4;
    stats.kind = 99;
    CHECK(xila_file_get_statistics(file, &stats) == XILA_STATUS_INVALID_ARGUMENT && stats.kind == 99);
    stats.size = sizeof stats;
    CHECK(xila_file_set_size(file, UINT64_MAX) == XILA_STATUS_OVERFLOW);
    CHECK(xila_file_set_size(file, 2) == XILA_STATUS_OK);
    CHECK(xila_file_seek(file, 0, XILA_SEEK_CURRENT, &position) == XILA_STATUS_OK && position == 6);
    CHECK(xila_file_get_statistics(file, &stats) == XILA_STATUS_OK && stats.file_size == 2);
    CHECK(xila_file_set_size(file, 8) == XILA_STATUS_OK);
    CHECK(xila_file_get_statistics(file, &stats) == XILA_STATUS_OK && stats.file_size == 8);
    CHECK(xila_file_seek(file, 0, XILA_SEEK_START, NULL) == XILA_STATUS_OK);
    CHECK(xila_file_read(file, names, sizeof names, &length) == XILA_STATUS_OK && length == 8);
    CHECK(memcmp(names, "ab\0\0\0\0\0\0", 8) == 0);
    CHECK(xila_file_close(file) == XILA_STATUS_OK);
    CHECK(xila_file_open(read_only, "alpha", 5, XILA_OPEN_READ, &file) == XILA_STATUS_OK);
    CHECK(xila_file_set_size(file, 0) == XILA_STATUS_PERMISSION_DENIED);
    CHECK(xila_file_close(file) == XILA_STATUS_OK);
    CHECK(xila_directory_create(directory, "entries/beta", 12) == XILA_STATUS_OK);
    CHECK(xila_directory_remove(directory, "entries", 7, 0) == XILA_STATUS_IS_A_DIRECTORY);
    CHECK(xila_directory_remove(directory, "entries", 7, XILA_REMOVE_DIRECTORY) == XILA_STATUS_DIRECTORY_NOT_EMPTY);
    CHECK(xila_directory_iterator_open(read_only, "", 0, &iterator) == XILA_STATUS_OK);
    kind = 99;
    CHECK(xila_directory_iterator_next(iterator, NULL, 0, &length, &kind) == XILA_STATUS_BUFFER_TOO_SMALL && length == 5 && kind == 99);
    CHECK(xila_directory_iterator_next(iterator, names, 1, &length, &kind) == XILA_STATUS_BUFFER_TOO_SMALL && length == 5);
    CHECK(xila_directory_iterator_next(iterator, names, sizeof names, &length, &kind) == XILA_STATUS_OK && length == 5 && memcmp(names, "alpha", 5) == 0 && kind == XILA_KIND_FILE);
    CHECK(xila_directory_iterator_rewind(iterator) == XILA_STATUS_OK);
    CHECK(xila_directory_iterator_next(iterator, names, sizeof names, &length, &kind) == XILA_STATUS_OK && length == 5 && memcmp(names, "alpha", 5) == 0);
    CHECK(xila_directory_iterator_next(iterator, names, sizeof names, &length, &kind) == XILA_STATUS_OK && length == 4 && memcmp(names, "beta", 4) == 0 && kind == XILA_KIND_DIRECTORY);
    length = 99;
    kind = 99;
    CHECK(xila_directory_iterator_next(iterator, names, sizeof names, &length, &kind) == XILA_STATUS_END && length == 99 && kind == 99);
    CHECK(xila_directory_iterator_close(iterator) == XILA_STATUS_OK);
    CHECK(xila_directory_rename(directory, "entries/alpha", 13, directory, "entries/gamma", 13) == XILA_STATUS_OK);
    CHECK(xila_directory_remove(directory, "entries/alpha", 13, 0) == XILA_STATUS_NOT_FOUND);
    CHECK(xila_directory_rename(directory, "entries/alpha", 13, directory, "entries/delta", 13) == XILA_STATUS_NOT_FOUND);
    CHECK(xila_directory_remove(directory, "entries/beta", 12, XILA_REMOVE_DIRECTORY) == XILA_STATUS_OK);
    CHECK(xila_directory_remove(directory, "entries/gamma", 13, 0) == XILA_STATUS_OK);
    xila_directory_close(read_only);
    xila_directory_close(write_only);
    CHECK(xila_directory_remove(directory, "entries", 7, XILA_REMOVE_DIRECTORY) == XILA_STATUS_OK);
    CHECK(xila_directory_create(directory, "missing/child", 13) == XILA_STATUS_NOT_FOUND);
    return 0;
}

extern void application_c_pause(void);
static XilaStatus receive_udp(XilaUdp *socket, char *buffer, size_t size,
                              XilaNetworkAddress *address, size_t *count) {
    for (unsigned i = 0; i < 2000; ++i) {
        XilaStatus status = xila_udp_receive_from(socket, buffer, size, address, count);
        if (status != XILA_STATUS_WOULD_BLOCK) return status;
        application_c_pause();
    }
    return XILA_STATUS_TIMED_OUT;
}

int application_c_network(const XilaNetwork *network, const XilaNetwork *denied,
                          const XilaNetwork *listen_only) {
    XilaNetworkAddress address = {XILA_ADDRESS_IPV4, {127, 0, 0, 1}, 52001, 0, 0};
    XilaNetworkAddress endpoint = {0};
    XilaTcp *client = NULL, *server = NULL;
    XilaTcpListener *listener = NULL;
    XilaUdp *sender = NULL, *receiver = NULL;
    size_t count = 99;
    char buffer[8] = {0};
    _Static_assert(sizeof(XilaNetworkAddress) == 28, "address layout");
    if (xila_tcp_connect(denied, &address, 1, &client) != XILA_STATUS_PERMISSION_DENIED || client != NULL) return 10;
    if (xila_tcp_listen(denied, &address, &listener) != XILA_STATUS_PERMISSION_DENIED) return 11;
    if (xila_udp_bind(denied, &address, &sender) != XILA_STATUS_PERMISSION_DENIED) return 12;
    if (xila_network_resolve(denied, "127.0.0.1", 9, XILA_ADDRESS_IPV4, &endpoint, &count) != XILA_STATUS_PERMISSION_DENIED || count != 99) return 13;
    address.family = 99;
    if (xila_tcp_connect(network, &address, 1, &client) != XILA_STATUS_INVALID_ARGUMENT) return 14;
    address.family = XILA_ADDRESS_IPV4;
    address.scope_id = 1;
    if (xila_udp_bind(network, &address, &sender) != XILA_STATUS_INVALID_ARGUMENT) return 15;
    address.scope_id = 0;
    if (xila_tcp_listen(network, &address, &listener) != XILA_STATUS_OK) return 16;
    if (xila_tcp_connect(network, &address, 2000, &client) != XILA_STATUS_OK) return 17;
    if (xila_tcp_accept(listener, &server) != XILA_STATUS_OK) return 18;
    if (xila_tcp_local_address(server, &endpoint) != XILA_STATUS_OK || memcmp(&address, &endpoint, sizeof address)) return 19;
    if (xila_tcp_peer_address(client, &endpoint) != XILA_STATUS_OK || memcmp(&address, &endpoint, sizeof address)) return 20;
    if (xila_tcp_send(client, "ping", 4, &count) != XILA_STATUS_OK || count != 4) return 21;
    if (xila_tcp_receive(server, buffer, sizeof buffer, &count) != XILA_STATUS_OK || count != 4 || memcmp(buffer, "ping", 4)) return 22;
    if (xila_tcp_send(server, buffer, count, &count) != XILA_STATUS_OK || count != 4) return 23;
    if (xila_tcp_receive(client, buffer, sizeof buffer, &count) != XILA_STATUS_OK || count != 4 || memcmp(buffer, "ping", 4)) return 24;
    if (xila_tcp_send(client, NULL, 0, &count) != XILA_STATUS_OK || count) return 25;
    if (xila_tcp_shutdown(client, 99) != XILA_STATUS_INVALID_ARGUMENT) return 26;
    if (xila_tcp_shutdown(client, XILA_SHUTDOWN_WRITE) != XILA_STATUS_OK) return 27;
    if (xila_tcp_receive(server, buffer, sizeof buffer, &count) != XILA_STATUS_OK || count) return 28;
    if (xila_tcp_close(client) || xila_tcp_close(server) || xila_tcp_listener_close(listener)) return 29;
    address.port = 52002;
    if (xila_udp_bind(listen_only, &address, &receiver) != XILA_STATUS_OK) return 30;
    count = 99;
    if (xila_udp_send_to(receiver, "x", 1, &address, &count) != XILA_STATUS_PERMISSION_DENIED || count != 99) return 31;
    if (xila_udp_receive_from(receiver, buffer, sizeof buffer, &endpoint, &count) != XILA_STATUS_WOULD_BLOCK || count != 99) return 32;
    address.port = 52003;
    if (xila_udp_bind(network, &address, &sender) != XILA_STATUS_OK) return 33;
    address.port = 52002;
    if (xila_udp_send_to(sender, "udp", 3, &address, &count) != XILA_STATUS_OK || count != 3) return 34;
    if (receive_udp(receiver, buffer, sizeof buffer, &endpoint, &count) != XILA_STATUS_OK || count != 3 || memcmp(buffer, "udp", 3) || endpoint.port != 52003) return 35;
    /* A LISTEN-only socket may receive, but needs CONNECT to send. */
    if (xila_udp_close(receiver) || xila_udp_close(sender)) return 36;
    if (xila_udp_bind(network, &address, &receiver) != XILA_STATUS_OK) return 37;
    address.port = 52003;
    if (xila_udp_bind(network, &address, &sender) != XILA_STATUS_OK) return 38;
    address.port = 52002;
    if (xila_udp_send_to(sender, "echo", 4, &address, &count)) return 39;
    count = 99;
    if (receive_udp(receiver, buffer, 1, &endpoint, &count) != XILA_STATUS_OVERFLOW || count != 99) return 47;
    if (receive_udp(receiver, buffer, sizeof buffer, &endpoint, &count) || count != 4) return 40;
    if (xila_udp_send_to(receiver, buffer, count, &endpoint, &count)) return 41;
    if (receive_udp(sender, buffer, sizeof buffer, &endpoint, &count) || count != 4 || memcmp(buffer, "echo", 4)) return 42;
    if (xila_udp_close(receiver) || xila_udp_close(sender)) return 43;
    count = 0;
    if (xila_network_resolve(network, "127.0.0.1", 9, XILA_ADDRESS_IPV4, NULL, &count) || count != 1) return 44;
    if (xila_network_resolve(network, "127.0.0.1", 9, XILA_ADDRESS_IPV4, &endpoint, &count) || count != 1 || endpoint.port != 0 || endpoint.bytes[0] != 127) return 45;
    /* Keep unresolved-neighbor discovery from affecting the traffic tests. */
    address.bytes[3] = 2;
    client = NULL;
    if (xila_tcp_connect(network, &address, 10, &client) != XILA_STATUS_TIMED_OUT || client != NULL) return 48;
    if (xila_tcp_close(NULL) || xila_tcp_listener_close(NULL) || xila_udp_close(NULL) || xila_network_close(NULL)) return 46;
    return 0;
}
