/* ENOSYS stubs for POSIX calls whose implementations are pending. */
#include <sys/types.h>
#include <sys/socket.h>
#include <unistd.h>

#include "internal.h"

pid_t fork(void) { return xila_fail_errno(ENOSYS); } /* never provided: no MMU */

/* ---- sockets: core#717 grants the network capability ---- */
#define UNSUPPORTED_SOCKET_CALL return xila_fail_errno(ENOSYS)
int socket(int domain, int type, int protocol) {
    (void)domain;
    (void)type;
    (void)protocol;
    UNSUPPORTED_SOCKET_CALL;
}
int connect(int fd, const struct sockaddr *address, socklen_t length) {
    (void)fd;
    (void)address;
    (void)length;
    UNSUPPORTED_SOCKET_CALL;
}
int bind(int fd, const struct sockaddr *address, socklen_t length) {
    (void)fd;
    (void)address;
    (void)length;
    UNSUPPORTED_SOCKET_CALL;
}
int listen(int fd, int backlog) {
    (void)fd;
    (void)backlog;
    UNSUPPORTED_SOCKET_CALL;
}
int accept(int fd, struct sockaddr *address, socklen_t *length) {
    (void)fd;
    (void)address;
    (void)length;
    UNSUPPORTED_SOCKET_CALL;
}
int shutdown(int fd, int how) {
    (void)fd;
    (void)how;
    UNSUPPORTED_SOCKET_CALL;
}
ssize_t send(int fd, const void *buffer, size_t size, int flags) {
    (void)fd;
    (void)buffer;
    (void)size;
    (void)flags;
    UNSUPPORTED_SOCKET_CALL;
}
ssize_t recv(int fd, void *buffer, size_t size, int flags) {
    (void)fd;
    (void)buffer;
    (void)size;
    (void)flags;
    UNSUPPORTED_SOCKET_CALL;
}
ssize_t sendto(int fd, const void *buffer, size_t size, int flags, const struct sockaddr *address,
               socklen_t length) {
    (void)fd;
    (void)buffer;
    (void)size;
    (void)flags;
    (void)address;
    (void)length;
    UNSUPPORTED_SOCKET_CALL;
}
ssize_t recvfrom(int fd, void *buffer, size_t size, int flags, struct sockaddr *address,
                 socklen_t *length) {
    (void)fd;
    (void)buffer;
    (void)size;
    (void)flags;
    (void)address;
    (void)length;
    UNSUPPORTED_SOCKET_CALL;
}
