/* Internet address families for the SDK's sockets. */
#ifndef _XILA_NETINET_IN_H
#define _XILA_NETINET_IN_H

#include <endian.h>
#include <stdint.h>
#include <sys/socket.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint16_t in_port_t;
typedef uint32_t in_addr_t;

/* Network byte order. */
struct in_addr {
    in_addr_t s_addr;
};

struct sockaddr_in {
    sa_family_t sin_family;
    in_port_t sin_port;
    struct in_addr sin_addr;
    unsigned char sin_zero[8];
};

struct in6_addr {
    union {
        uint8_t __s6_addr[16];
        uint16_t __s6_addr16[8];
        uint32_t __s6_addr32[4];
    } __in6_union;
};
#define s6_addr __in6_union.__s6_addr

/* Core has no scoped IPv6 endpoints: sin6_scope_id must be zero. */
struct sockaddr_in6 {
    sa_family_t sin6_family;
    in_port_t sin6_port;
    uint32_t sin6_flowinfo;
    struct in6_addr sin6_addr;
    uint32_t sin6_scope_id;
};

#define IPPROTO_IP 0
#define IPPROTO_TCP 6
#define IPPROTO_UDP 17
#define IPPROTO_IPV6 41

/* Host byte order, like their POSIX definitions: pass them through htonl. */
#define INADDR_ANY ((in_addr_t)0x00000000)
#define INADDR_BROADCAST ((in_addr_t)0xffffffff)
#define INADDR_NONE ((in_addr_t)0xffffffff)
#define INADDR_LOOPBACK ((in_addr_t)0x7f000001)

#define IN6ADDR_ANY_INIT {{{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0}}}
#define IN6ADDR_LOOPBACK_INIT {{{0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1}}}
extern const struct in6_addr in6addr_any;
extern const struct in6_addr in6addr_loopback;

#define INET_ADDRSTRLEN 16
#define INET6_ADDRSTRLEN 46

#ifndef htons
#define htons(x) htobe16(x)
#define ntohs(x) be16toh(x)
#define htonl(x) htobe32(x)
#define ntohl(x) be32toh(x)
#endif

#ifdef __cplusplus
}
#endif

#endif
