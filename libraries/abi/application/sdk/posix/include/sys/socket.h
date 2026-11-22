/* Sockets over the network capability that the host grants in XilaStartup.network. See
 * "Sockets and name resolution" in the SDK README for what is supported. */
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

/* Large and aligned enough for every address family the SDK supports. */
struct sockaddr_storage {
    sa_family_t ss_family;
    char __ss_padding[128 - sizeof(sa_family_t) - sizeof(uint64_t)];
    uint64_t __ss_align;
};

#define AF_UNSPEC 0
#define AF_INET 2
#define AF_INET6 10
#define PF_UNSPEC AF_UNSPEC
#define PF_INET AF_INET
#define PF_INET6 AF_INET6

#define SOCK_STREAM 1
#define SOCK_DGRAM 2
/* Flags that socket() accepts in its type argument, as on Linux. */
#define SOCK_NONBLOCK 0x800
#define SOCK_CLOEXEC 0x80000

#define SHUT_RD 0
#define SHUT_WR 1
#define SHUT_RDWR 2

/* A listener holds one pending connection: the backlog is accepted and ignored. */
#define SOMAXCONN 1

#define MSG_PEEK 0x2
#define MSG_DONTWAIT 0x40
#define MSG_NOSIGNAL 0x4000

#define SOL_SOCKET 1
#define SO_REUSEADDR 2
#define SO_TYPE 3
#define SO_ERROR 4
#define SO_KEEPALIVE 9

int socket(int domain, int type, int protocol);
int connect(int fd, const struct sockaddr *address, socklen_t length);
int bind(int fd, const struct sockaddr *address, socklen_t length);
int listen(int fd, int backlog);
int accept(int fd, struct sockaddr *address, socklen_t *length);
int shutdown(int fd, int how);
int getsockname(int fd, struct sockaddr *address, socklen_t *length);
int getpeername(int fd, struct sockaddr *address, socklen_t *length);
int getsockopt(int fd, int level, int name, void *value, socklen_t *length);
int setsockopt(int fd, int level, int name, const void *value, socklen_t length);
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
