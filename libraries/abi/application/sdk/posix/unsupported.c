/* Calls that need ABI slots which do not exist yet. They fail with ENOSYS so that
 * portable code can probe for them at run time. Each group names the work that
 * provides it:
 *   processes and environment (posix_spawn, waitpid, kill): core#717
 *   threads and synchronization (pthread_*):                  core#718
 *   readiness, pipes and descriptor duplication (poll, pipe): core#719
 *   sockets and name resolution: needs the startup network capability, core#717 */
#include <sys/types.h>
#include <poll.h>
#include <pthread.h>
#include <spawn.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

#include "internal.h"

/* ---- core#719 ---- */
int pipe(int fds[2]) {
    (void)fds;
    return xila_fail_errno(ENOSYS);
}
int poll(struct pollfd *fds, nfds_t count, int timeout) {
    (void)fds;
    (void)count;
    (void)timeout;
    return xila_fail_errno(ENOSYS);
}
int dup(int fd) {
    (void)fd;
    return xila_fail_errno(ENOSYS);
}
int dup2(int fd, int target) {
    (void)fd;
    (void)target;
    return xila_fail_errno(ENOSYS);
}

/* ---- core#717 ---- */
int posix_spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *actions,
                const posix_spawnattr_t *attributes, char *const arguments[],
                char *const environment[]) {
    (void)pid;
    (void)path;
    (void)actions;
    (void)attributes;
    (void)arguments;
    (void)environment;
    return ENOSYS;
}
pid_t waitpid(pid_t pid, int *status, int options) {
    (void)pid;
    (void)status;
    (void)options;
    return xila_fail_errno(ENOSYS);
}
pid_t fork(void) { return xila_fail_errno(ENOSYS); } /* never provided: no MMU */

/* ---- core#718 ---- */
int pthread_create(pthread_t *thread, const pthread_attr_t *attributes, void *(*entry)(void *),
                   void *argument) {
    (void)thread;
    (void)attributes;
    (void)entry;
    (void)argument;
    return ENOSYS;
}
int pthread_join(pthread_t thread, void **result) {
    (void)thread;
    (void)result;
    return ENOSYS;
}
int pthread_detach(pthread_t thread) {
    (void)thread;
    return ENOSYS;
}

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
