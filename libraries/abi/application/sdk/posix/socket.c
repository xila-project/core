/* Sockets over the network capability that the host grants in XilaStartup.network.
 *
 * A socket descriptor points to a small record on the process heap, shared by every
 * duplicate of the descriptor. The record holds the native object once there is one:
 * a XilaTcp for a connected stream socket, a XilaTcpListener for a listening one and a
 * XilaUdp for a bound datagram socket. Core binds and listens in one call, so a stream
 * socket keeps its bound address until listen(). The descriptor table reaches this file
 * only through weak references (internal.h): an image that never calls socket() or
 * accept() does not link it. */
#define XILA_SOCKET_DEFINITIONS
#include <arpa/inet.h>
#include <fcntl.h>
#include <limits.h>
#include <netinet/tcp.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#include "internal.h"

enum { STATE_OPEN, STATE_BOUND, STATE_LISTENING, STATE_CONNECTED };

#define SHUT_READ_DONE 1
#define SHUT_WRITE_DONE 2

/* The ephemeral port range of RFC 6335, from which an unbound datagram socket gets
   its port on its first send. */
#define EPHEMERAL_FIRST 49152u
#define EPHEMERAL_COUNT 16384u
#define EPHEMERAL_ATTEMPTS 8

struct xila_socket {
    void *handle;
    /* The bound address: a stream socket's until and after listen, a datagram
       socket's once it is bound. */
    XilaNetworkAddress local;
    /* A datagram socket's default destination, set by connect(). */
    XilaNetworkAddress peer;
    uint16_t references;
    uint8_t type;
    uint8_t family;
    uint8_t state;
    uint8_t nonblocking;
    uint8_t has_peer;
    uint8_t shut;
};

static const XilaNetwork *network(void) {
    return xila_process.startup != NULL ? xila_process.startup->network : NULL;
}

static struct xila_socket *socket_of(int fd) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return NULL;
    if (entry->kind != XILA_FD_SOCKET) {
        errno = ENOTSOCK;
        return NULL;
    }
    return entry->object;
}

/* ---- descriptor table hooks ----------------------------------------------------- */

void xila_socket_retain(struct xila_socket *socket) { ++socket->references; }

XilaStatus xila_socket_release(struct xila_socket *socket) {
    if (--socket->references != 0)
        return XILA_STATUS_OK;
    XilaStatus status = XILA_STATUS_OK;
    if (socket->handle != NULL) {
        if (socket->type == SOCK_DGRAM)
            status = xila_udp_close(socket->handle);
        else if (socket->state == STATE_LISTENING)
            status = xila_tcp_listener_close(socket->handle);
        else
            status = xila_tcp_close(socket->handle);
    }
    xila_heap_release(socket);
    return status;
}

void xila_socket_poll_entry(const struct xila_socket *socket, XilaPollEntry *entry) {
    entry->handle = socket->handle;
    if (socket->handle == NULL)
        entry->kind = XILA_POLL_IGNORE; /* nothing to wait for before bind or connect */
    else if (socket->type == SOCK_DGRAM)
        entry->kind = XILA_POLL_UDP;
    else
        entry->kind = socket->state == STATE_LISTENING ? XILA_POLL_TCP_LISTENER : XILA_POLL_TCP;
}

int xila_socket_get_flags(const struct xila_socket *socket) {
    return O_RDWR | (socket->nonblocking ? O_NONBLOCK : 0);
}

void xila_socket_set_flags(struct xila_socket *socket, int flags) {
    socket->nonblocking = (flags & O_NONBLOCK) != 0;
}

/* ---- addresses ------------------------------------------------------------------ */

static const uint8_t v4_mapped_prefix[12] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff};

static sa_family_t family_of(const struct sockaddr *address) {
    sa_family_t family;
    memcpy(&family, &address->sa_family, sizeof(family));
    return family;
}

/* Convert a POSIX address of the socket's family. A socket keeps its family, as with
   IPV6_V6ONLY: an AF_INET6 socket does not reach IPv4 peers, so an IPv4-mapped address
   is unreachable. Returns 0 or an errno value. */
static int native_address(const struct xila_socket *socket, const struct sockaddr *address,
                          socklen_t length, XilaNetworkAddress *native) {
    if (address == NULL)
        return EFAULT;
    if (length < sizeof(sa_family_t))
        return EINVAL;
    memset(native, 0, sizeof(*native));
    if (family_of(address) != socket->family)
        return EAFNOSUPPORT;
    if (socket->family == AF_INET) {
        struct sockaddr_in value;
        if (length < sizeof(value))
            return EINVAL;
        memcpy(&value, address, sizeof(value));
        native->family = XILA_ADDRESS_IPV4;
        memcpy(native->bytes, &value.sin_addr, 4);
        native->port = ntohs(value.sin_port);
        return 0;
    }
    struct sockaddr_in6 value;
    if (length < sizeof(value))
        return EINVAL;
    memcpy(&value, address, sizeof(value));
    native->port = ntohs(value.sin6_port);
    if (memcmp(value.sin6_addr.s6_addr, v4_mapped_prefix, sizeof(v4_mapped_prefix)) == 0)
        return ENETUNREACH;
    native->family = XILA_ADDRESS_IPV6;
    memcpy(native->bytes, value.sin6_addr.s6_addr, 16);
    native->scope_id = value.sin6_scope_id; /* Core refuses a nonzero scope */
    return 0;
}

/* Store `native` in the caller's buffer, truncated to `*length`, and set `*length` to
   the full size. Core reports only addresses of the socket's own family: a wildcard
   bind keeps its family. */
static void posix_address(const struct xila_socket *socket, const XilaNetworkAddress *native,
                          struct sockaddr *address, socklen_t *length) {
    union {
        struct sockaddr_in in;
        struct sockaddr_in6 in6;
    } value;
    memset(&value, 0, sizeof(value));
    socklen_t size;
    if (socket->family == AF_INET) {
        value.in.sin_family = AF_INET;
        value.in.sin_port = htons(native->port);
        memcpy(&value.in.sin_addr, native->bytes, 4);
        size = sizeof(value.in);
    } else {
        value.in6.sin6_family = AF_INET6;
        value.in6.sin6_port = htons(native->port);
        memcpy(value.in6.sin6_addr.s6_addr, native->bytes, 16);
        size = sizeof(value.in6);
    }
    memcpy(address, &value, *length < size ? *length : size);
    *length = size;
}

static int same_address(const XilaNetworkAddress *left, const XilaNetworkAddress *right) {
    return left->family == right->family && left->port == right->port &&
           memcmp(left->bytes, right->bytes, 16) == 0;
}

/* ---- readiness ------------------------------------------------------------------ */

/* Wait until the socket reports `events`, an error or a hangup. A zero timeout probes
   once. Returns 1 when ready, 0 when not, or -1 with errno set. */
static int wait_for(const struct xila_socket *socket, uint16_t events, int32_t timeout) {
    XilaPollEntry entry = {0};
    xila_socket_poll_entry(socket, &entry);
    entry.events = events;
    size_t ready = 0;
    XilaStatus status = xila_poll_wait(&entry, 1, timeout, &ready);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    return entry.revents != 0;
}

/* For a nonblocking call: proceed only if the socket is ready now. Returns 0 to
   proceed, or -1 with errno set (EAGAIN when it is not ready). */
static int ready_now(const struct xila_socket *socket, uint16_t events) {
    int ready = wait_for(socket, events, 0);
    if (ready < 0)
        return -1;
    return ready ? 0 : xila_fail_errno(EAGAIN);
}

/* ---- creation, binding and connection ------------------------------------------- */

int socket(int domain, int type, int protocol) {
    int flags = type & (SOCK_NONBLOCK | SOCK_CLOEXEC);
    type &= ~(SOCK_NONBLOCK | SOCK_CLOEXEC);
    if (domain != AF_INET && domain != AF_INET6)
        return xila_fail_errno(EAFNOSUPPORT);
    if (type != SOCK_STREAM && type != SOCK_DGRAM)
        return xila_fail_errno(EPROTOTYPE);
    if (protocol != 0 && protocol != (type == SOCK_STREAM ? IPPROTO_TCP : IPPROTO_UDP))
        return xila_fail_errno(EPROTONOSUPPORT);
    if (network() == NULL)
        return xila_fail_errno(EACCES); /* the launch granted no network rights */

    struct xila_socket *created = xila_heap_alloc(sizeof(*created));
    if (created == NULL)
        return -1;
    *created = (struct xila_socket){
        .references = 1,
        .type = (uint8_t)type,
        .family = (uint8_t)domain,
        .nonblocking = (flags & SOCK_NONBLOCK) != 0,
    };
    int fd = xila_fd_allocate(created, XILA_FD_SOCKET, (flags & SOCK_CLOEXEC) ? XILA_FD_CLOEXEC : 0);
    if (fd < 0) {
        int error = errno;
        xila_heap_release(created);
        errno = error;
    }
    return fd;
}

static XilaStatus bind_datagram(struct xila_socket *socket, const XilaNetworkAddress *address) {
    XilaUdp *udp = NULL;
    XilaStatus status = xila_udp_bind(network(), address, &udp);
    if (status != XILA_STATUS_OK)
        return status;
    socket->handle = udp;
    socket->local = *address;
    socket->state = STATE_BOUND;
    return XILA_STATUS_OK;
}

int bind(int fd, const struct sockaddr *address, socklen_t length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (socket->state != STATE_OPEN)
        return xila_fail_errno(EINVAL);
    XilaNetworkAddress native;
    int error = native_address(socket, address, length, &native);
    if (error != 0)
        return xila_fail_errno(error);
    /* Core assigns no port to a bind or a listener: port zero is refused. */
    if (native.port == 0)
        return xila_fail_errno(EADDRNOTAVAIL);
    if (socket->type == SOCK_DGRAM) {
        XilaStatus status = bind_datagram(socket, &native);
        return status == XILA_STATUS_OK ? 0 : xila_fail(status);
    }
    socket->local = native;
    socket->state = STATE_BOUND;
    return 0;
}

/* Bind an unbound datagram socket to the wildcard address and an ephemeral port, as its
   first send does on other systems. Core does not report a port that another socket
   already uses, so the port is random rather than the next free one. */
static XilaStatus bind_ephemeral(struct xila_socket *socket) {
    XilaNetworkAddress wildcard = {
        .family = socket->family == AF_INET ? XILA_ADDRESS_IPV4 : XILA_ADDRESS_IPV6,
    };
    uint16_t offset = 0;
    XilaStatus status = xila_random_fill(&offset, sizeof(offset));
    if (status != XILA_STATUS_OK)
        offset = (uint16_t)(uintptr_t)socket;
    for (int attempt = 0; attempt < EPHEMERAL_ATTEMPTS; ++attempt) {
        wildcard.port = (uint16_t)(EPHEMERAL_FIRST + (offset + attempt * 97u) % EPHEMERAL_COUNT);
        status = bind_datagram(socket, &wildcard);
        if (status != XILA_STATUS_ADDRESS_IN_USE)
            break;
    }
    return status;
}

int listen(int fd, int backlog) {
    (void)backlog; /* Core's listener holds one pending connection */
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (socket->type != SOCK_STREAM)
        return xila_fail_errno(EOPNOTSUPP);
    if (socket->state == STATE_LISTENING)
        return 0;
    if (socket->state == STATE_CONNECTED)
        return xila_fail_errno(EINVAL);
    if (socket->state != STATE_BOUND)
        return xila_fail_errno(EDESTADDRREQ); /* Core has no ephemeral listener port */
    XilaTcpListener *listener = NULL;
    XilaStatus status = xila_tcp_listen(network(), &socket->local, &listener);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    socket->handle = listener;
    socket->state = STATE_LISTENING;
    return 0;
}

int accept(int fd, struct sockaddr *address, socklen_t *length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (socket->type != SOCK_STREAM)
        return xila_fail_errno(EOPNOTSUPP);
    if (socket->state != STATE_LISTENING)
        return xila_fail_errno(EINVAL);
    if (address != NULL && length == NULL)
        return xila_fail_errno(EFAULT);
    if (socket->nonblocking && ready_now(socket, XILA_POLL_IN) != 0)
        return -1;

    /* Allocate the record first, so running out of memory does not drop a connection. */
    struct xila_socket *accepted = xila_heap_alloc(sizeof(*accepted));
    if (accepted == NULL)
        return -1;
    XilaTcp *connection = NULL;
    XilaStatus status = xila_tcp_accept(socket->handle, &connection);
    if (status != XILA_STATUS_OK) {
        xila_heap_release(accepted);
        return xila_fail(status);
    }
    *accepted = (struct xila_socket){
        .handle = connection,
        .references = 1,
        .type = SOCK_STREAM,
        .family = socket->family,
        .state = STATE_CONNECTED,
    };
    int accepted_fd = xila_fd_allocate(accepted, XILA_FD_SOCKET, 0);
    if (accepted_fd < 0) {
        int error = errno;
        (void)xila_socket_release(accepted);
        return xila_fail_errno(error);
    }
    if (address != NULL) {
        XilaNetworkAddress peer;
        if (xila_tcp_peer_address(connection, &peer) == XILA_STATUS_OK)
            posix_address(accepted, &peer, address, length);
        else
            *length = 0;
    }
    return accepted_fd;
}

int connect(int fd, const struct sockaddr *address, socklen_t length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (socket->type == SOCK_DGRAM) {
        /* Only sets or clears the default destination. */
        if (address != NULL && length >= sizeof(sa_family_t) && family_of(address) == AF_UNSPEC) {
            socket->has_peer = 0;
            return 0;
        }
        XilaNetworkAddress peer;
        int error = native_address(socket, address, length, &peer);
        if (error != 0)
            return xila_fail_errno(error);
        socket->peer = peer;
        socket->has_peer = 1;
        return 0;
    }
    if (socket->state == STATE_CONNECTED)
        return xila_fail_errno(EISCONN);
    if (socket->state == STATE_LISTENING)
        return xila_fail_errno(EINVAL);
    XilaNetworkAddress peer;
    int error = native_address(socket, address, length, &peer);
    if (error != 0)
        return xila_fail_errno(error);
    /* Core picks the local endpoint: an address given to bind() is not used. */
    XilaTcp *connection = NULL;
    XilaStatus status = xila_tcp_connect(network(), &peer, 0, &connection);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    socket->handle = connection;
    socket->state = STATE_CONNECTED;
    return 0;
}

int shutdown(int fd, int how) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (how != SHUT_RD && how != SHUT_WR && how != SHUT_RDWR)
        return xila_fail_errno(EINVAL);
    int connected = socket->type == SOCK_STREAM ? socket->state == STATE_CONNECTED
                                                : socket->has_peer;
    if (!connected)
        return xila_fail_errno(ENOTCONN);
    if (how != SHUT_RD && socket->type == SOCK_STREAM && !(socket->shut & SHUT_WRITE_DONE)) {
        XilaStatus status = xila_tcp_shutdown(socket->handle, XILA_SHUTDOWN_WRITE);
        if (status != XILA_STATUS_OK)
            return xila_fail(status);
    }
    if (how != SHUT_WR)
        socket->shut |= SHUT_READ_DONE;
    if (how != SHUT_RD)
        socket->shut |= SHUT_WRITE_DONE;
    return 0;
}

/* ---- names and options ---------------------------------------------------------- */

int getsockname(int fd, struct sockaddr *address, socklen_t *length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (address == NULL || length == NULL)
        return xila_fail_errno(EFAULT);
    XilaNetworkAddress local = socket->local;
    if (socket->type == SOCK_STREAM && socket->state == STATE_CONNECTED) {
        XilaStatus status = xila_tcp_local_address(socket->handle, &local);
        if (status != XILA_STATUS_OK)
            return xila_fail(status);
    } else if (socket->state == STATE_OPEN) {
        local = (XilaNetworkAddress){0}; /* the unspecified address, port zero */
        local.family = socket->family == AF_INET ? XILA_ADDRESS_IPV4 : XILA_ADDRESS_IPV6;
    }
    posix_address(socket, &local, address, length);
    return 0;
}

int getpeername(int fd, struct sockaddr *address, socklen_t *length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (address == NULL || length == NULL)
        return xila_fail_errno(EFAULT);
    XilaNetworkAddress peer;
    if (socket->type == SOCK_DGRAM) {
        if (!socket->has_peer)
            return xila_fail_errno(ENOTCONN);
        peer = socket->peer;
    } else {
        if (socket->state != STATE_CONNECTED)
            return xila_fail_errno(ENOTCONN);
        XilaStatus status = xila_tcp_peer_address(socket->handle, &peer);
        if (status != XILA_STATUS_OK)
            return xila_fail(status);
    }
    posix_address(socket, &peer, address, length);
    return 0;
}

/* Options that are accepted and have no effect. */
static int ignored_option(int level, int name) {
    return (level == SOL_SOCKET && (name == SO_REUSEADDR || name == SO_KEEPALIVE)) ||
           (level == IPPROTO_TCP && name == TCP_NODELAY);
}

int setsockopt(int fd, int level, int name, const void *value, socklen_t length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (!ignored_option(level, name))
        return xila_fail_errno(ENOPROTOOPT);
    if (value == NULL)
        return xila_fail_errno(EFAULT);
    if (length < sizeof(int))
        return xila_fail_errno(EINVAL);
    return 0;
}

int getsockopt(int fd, int level, int name, void *value, socklen_t *length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    int result;
    if (level == SOL_SOCKET && name == SO_TYPE)
        result = socket->type;
    else if ((level == SOL_SOCKET && name == SO_ERROR) || ignored_option(level, name))
        result = 0; /* errors are reported by the call that meets them */
    else
        return xila_fail_errno(ENOPROTOOPT);
    if (value == NULL || length == NULL)
        return xila_fail_errno(EFAULT);
    if (*length < sizeof(result))
        return xila_fail_errno(EINVAL);
    memcpy(value, &result, sizeof(result));
    *length = sizeof(result);
    return 0;
}

/* ---- data transfer -------------------------------------------------------------- */

static ssize_t stream_send(struct xila_socket *socket, const void *buffer, size_t size,
                           int nonblocking) {
    if (socket->state != STATE_CONNECTED)
        return xila_fail_errno(ENOTCONN);
    if (socket->shut & SHUT_WRITE_DONE)
        return xila_fail_errno(EPIPE); /* there are no signals: no SIGPIPE either */
    if (nonblocking && size != 0 && ready_now(socket, XILA_POLL_OUT) != 0)
        return -1;
    size_t count = 0;
    XilaStatus status = xila_tcp_send(socket->handle, buffer, size, &count);
    return status == XILA_STATUS_OK ? (ssize_t)count : xila_fail(status);
}

static ssize_t stream_receive(struct xila_socket *socket, void *buffer, size_t size,
                              int nonblocking) {
    if (socket->state != STATE_CONNECTED)
        return xila_fail_errno(ENOTCONN);
    if (socket->shut & SHUT_READ_DONE)
        return 0;
    if (nonblocking && size != 0 && ready_now(socket, XILA_POLL_IN) != 0)
        return -1;
    size_t count = 0;
    XilaStatus status = xila_tcp_receive(socket->handle, buffer, size, &count);
    return status == XILA_STATUS_OK ? (ssize_t)count : xila_fail(status);
}

static ssize_t datagram_send(struct xila_socket *socket, const void *buffer, size_t size,
                             int nonblocking, const XilaNetworkAddress *destination) {
    if (socket->shut & SHUT_WRITE_DONE)
        return xila_fail_errno(EPIPE);
    if (destination == NULL) {
        if (!socket->has_peer)
            return xila_fail_errno(EDESTADDRREQ);
        destination = &socket->peer;
    }
    if (socket->handle == NULL) {
        XilaStatus status = bind_ephemeral(socket);
        if (status != XILA_STATUS_OK)
            return xila_fail(status);
    }
    for (;;) {
        size_t count = 0;
        XilaStatus status = xila_udp_send_to(socket->handle, buffer, size, destination, &count);
        if (status == XILA_STATUS_OK)
            return (ssize_t)count;
        if (status == XILA_STATUS_OVERFLOW)
            return xila_fail_errno(EMSGSIZE);
        if (status != XILA_STATUS_WOULD_BLOCK || nonblocking)
            return xila_fail(status);
        if (wait_for(socket, XILA_POLL_OUT, -1) < 0)
            return -1;
    }
}

/* Receive one datagram that does not fit the caller's buffer: copy what fits and drop
   the rest, as POSIX does. Core keeps such a datagram queued, so it is read into a
   larger temporary buffer first. */
static XilaStatus receive_truncated(struct xila_socket *socket, void *buffer, size_t size,
                                    XilaNetworkAddress *source, size_t *count) {
    for (size_t capacity = 512; capacity <= 65536; capacity *= 2) {
        if (capacity <= size)
            continue;
        void *staging = xila_heap_alloc(capacity);
        if (staging == NULL)
            return XILA_STATUS_OUT_OF_MEMORY;
        size_t received = 0;
        XilaStatus status = xila_udp_receive_from(socket->handle, staging, capacity, source,
                                                  &received);
        if (status == XILA_STATUS_OK) {
            *count = received < size ? received : size;
            if (*count != 0)
                memcpy(buffer, staging, *count);
        }
        xila_heap_release(staging);
        if (status != XILA_STATUS_OVERFLOW)
            return status;
    }
    return XILA_STATUS_MESSAGE_TOO_LARGE;
}

static ssize_t datagram_receive(struct xila_socket *socket, void *buffer, size_t size,
                                int nonblocking, XilaNetworkAddress *source) {
    if (socket->shut & SHUT_READ_DONE)
        return 0;
    if (socket->handle == NULL)
        return nonblocking ? xila_fail_errno(EAGAIN) : xila_fail_errno(ENOTCONN);
    for (;;) {
        size_t count = 0;
        XilaStatus status = xila_udp_receive_from(socket->handle, buffer, size, source, &count);
        if (status == XILA_STATUS_OVERFLOW)
            status = receive_truncated(socket, buffer, size, source, &count);
        if (status == XILA_STATUS_OK) {
            /* A connected datagram socket only receives from its peer. */
            if (socket->has_peer && !same_address(source, &socket->peer))
                continue;
            return (ssize_t)count;
        }
        if (status != XILA_STATUS_WOULD_BLOCK || nonblocking)
            return xila_fail(status);
        if (wait_for(socket, XILA_POLL_IN, -1) < 0)
            return -1;
    }
}

static int nonblocking(const struct xila_socket *socket, int flags) {
    return socket->nonblocking || (flags & MSG_DONTWAIT);
}

ssize_t sendto(int fd, const void *buffer, size_t size, int flags, const struct sockaddr *address,
               socklen_t length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (flags & ~(MSG_DONTWAIT | MSG_NOSIGNAL))
        return xila_fail_errno(EOPNOTSUPP);
    if (size > SSIZE_MAX)
        return xila_fail_errno(EINVAL);
    if (socket->type == SOCK_STREAM)
        return stream_send(socket, buffer, size, nonblocking(socket, flags)); /* address ignored */
    if (address == NULL)
        return datagram_send(socket, buffer, size, nonblocking(socket, flags), NULL);
    XilaNetworkAddress destination;
    int error = native_address(socket, address, length, &destination);
    if (error != 0)
        return xila_fail_errno(error);
    return datagram_send(socket, buffer, size, nonblocking(socket, flags), &destination);
}

ssize_t send(int fd, const void *buffer, size_t size, int flags) {
    return sendto(fd, buffer, size, flags, NULL, 0);
}

ssize_t recvfrom(int fd, void *buffer, size_t size, int flags, struct sockaddr *address,
                 socklen_t *length) {
    struct xila_socket *socket = socket_of(fd);
    if (socket == NULL)
        return -1;
    if (flags & ~MSG_DONTWAIT)
        return xila_fail_errno(EOPNOTSUPP);
    if (size > SSIZE_MAX)
        return xila_fail_errno(EINVAL);
    if (address != NULL && length == NULL)
        return xila_fail_errno(EFAULT);
    if (socket->type == SOCK_STREAM) {
        if (address != NULL)
            *length = 0; /* a connection's source is its peer: see getpeername() */
        return stream_receive(socket, buffer, size, nonblocking(socket, flags));
    }
    XilaNetworkAddress source;
    ssize_t count = datagram_receive(socket, buffer, size, nonblocking(socket, flags), &source);
    if (count >= 0 && address != NULL)
        posix_address(socket, &source, address, length);
    return count;
}

ssize_t recv(int fd, void *buffer, size_t size, int flags) {
    return recvfrom(fd, buffer, size, flags, NULL, NULL);
}

ssize_t xila_socket_read(struct xila_socket *socket, void *buffer, size_t size) {
    if (socket->type == SOCK_STREAM)
        return stream_receive(socket, buffer, size, socket->nonblocking);
    XilaNetworkAddress source;
    return datagram_receive(socket, buffer, size, socket->nonblocking, &source);
}

ssize_t xila_socket_write(struct xila_socket *socket, const void *buffer, size_t size) {
    if (socket->type == SOCK_STREAM)
        return stream_send(socket, buffer, size, socket->nonblocking);
    return datagram_send(socket, buffer, size, socket->nonblocking, NULL);
}
