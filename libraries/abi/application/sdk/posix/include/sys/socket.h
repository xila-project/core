/* Declarations for the socket calls. Every one fails with ENOSYS until the native ABI
 * grants a network capability to applications (core#717). */
#ifndef _XILA_SYS_SOCKET_H
#define _XILA_SYS_SOCKET_H

#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint32_t socklen_t;
typedef uint16_t sa_family_t;

struct sockaddr {
    sa_family_t sa_family;
    char sa_data[14];
};

#define AF_UNSPEC 0
#define AF_INET 2
#define AF_INET6 10
#define SOCK_STREAM 1
#define SOCK_DGRAM 2
#define SHUT_RD 0
#define SHUT_WR 1
#define SHUT_RDWR 2

int socket(int domain, int type, int protocol);
int connect(int fd, const struct sockaddr *address, socklen_t length);
int bind(int fd, const struct sockaddr *address, socklen_t length);
int listen(int fd, int backlog);
int accept(int fd, struct sockaddr *address, socklen_t *length);
int shutdown(int fd, int how);
ssize_t send(int fd, const void *buffer, size_t size, int flags);
ssize_t recv(int fd, void *buffer, size_t size, int flags);
ssize_t sendto(int fd, const void *buffer, size_t size, int flags, const struct sockaddr *address,
               socklen_t length);
ssize_t recvfrom(int fd, void *buffer, size_t size, int flags, struct sockaddr *address,
                 socklen_t *length);

#ifdef __cplusplus
}
#endif

#endif
