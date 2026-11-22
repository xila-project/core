/* Probe for the SDK's sockets and name resolution, over the host's loopback interface.
 * The first argument selects the scenario; the host test (executable's tests/sdk.rs)
 * launches it with and without a network grant.
 *
 *   sockets     address text, getaddrinfo, a TCP exchange, UDP datagrams and errno values
 *   family      wildcard binds keep their address family
 *   no-network  what a process gets without network rights
 *   spawn       a parent granted connect and resolve passes on a subset, never more
 *   child       run by spawn: a child with connect only
 */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>
#include <xila/stubs.h>

#define CHECK(step, condition)                                                           \
    do {                                                                                 \
        if (!(condition)) {                                                              \
            fprintf(stderr, "network: %s failed: errno %d\n", step, errno);              \
            return 1;                                                                    \
        }                                                                                \
    } while (0)

#define TCP_PORT 7101
#define UDP_PORT 7102

static int text_round_trip(int family, const char *text, const char *expected) {
    unsigned char bytes[16];
    char back[INET6_ADDRSTRLEN];
    return inet_pton(family, text, bytes) == 1 &&
           inet_ntop(family, bytes, back, sizeof(back)) == back && strcmp(back, expected) == 0;
}

static int address_text(void) {
    CHECK("IPv4 text", text_round_trip(AF_INET, "192.0.2.1", "192.0.2.1"));
    CHECK("IPv6 loopback text", text_round_trip(AF_INET6, "::1", "::1"));
    CHECK("IPv6 any text", text_round_trip(AF_INET6, "::", "::"));
    CHECK("IPv6 compressed text", text_round_trip(AF_INET6, "2001:DB8:0:0:0:0:0:1", "2001:db8::1"));
    CHECK("IPv6 trailing gap", text_round_trip(AF_INET6, "fe80::", "fe80::"));
    CHECK("IPv6 single zero group", text_round_trip(AF_INET6, "1:0:2:3:4:5:6:7", "1:0:2:3:4:5:6:7"));
    CHECK("IPv4-mapped text", text_round_trip(AF_INET6, "::ffff:192.0.2.1", "::ffff:192.0.2.1"));
    unsigned char bytes[16];
    CHECK("short IPv4", inet_pton(AF_INET, "192.0.2", bytes) == 0);
    CHECK("leading zero", inet_pton(AF_INET, "192.0.2.01", bytes) == 0);
    CHECK("large part", inet_pton(AF_INET, "256.0.0.1", bytes) == 0);
    CHECK("two gaps", inet_pton(AF_INET6, "1::2::3", bytes) == 0);
    CHECK("nine groups", inet_pton(AF_INET6, "1:2:3:4:5:6:7:8:9", bytes) == 0);
    CHECK("unknown family", inet_pton(99, "1.2.3.4", bytes) == -1 && errno == EAFNOSUPPORT);
    char small[4];
    CHECK("small buffer", inet_ntop(AF_INET, bytes, small, sizeof(small)) == NULL &&
                              errno == ENOSPC);
    CHECK("inet_addr", inet_addr("127.0.0.1") == htonl(INADDR_LOOPBACK) &&
                           inet_addr("x") == INADDR_NONE);
    return 0;
}

static int resolution(void) {
    struct addrinfo hints = {.ai_family = AF_INET, .ai_socktype = SOCK_STREAM};
    struct addrinfo *list = NULL;
    CHECK("numeric getaddrinfo", getaddrinfo("127.0.0.1", "7101", &hints, &list) == 0);
    struct sockaddr_in address;
    memcpy(&address, list->ai_addr, sizeof(address));
    CHECK("numeric result", list->ai_next == NULL && list->ai_family == AF_INET &&
                                list->ai_socktype == SOCK_STREAM &&
                                list->ai_protocol == IPPROTO_TCP &&
                                list->ai_addrlen == sizeof(address) &&
                                address.sin_port == htons(7101) &&
                                address.sin_addr.s_addr == htonl(INADDR_LOOPBACK));
    freeaddrinfo(list);

    /* No socket type: one result per type. */
    hints = (struct addrinfo){.ai_family = AF_UNSPEC, .ai_flags = AI_CANONNAME};
    CHECK("localhost getaddrinfo", getaddrinfo("localhost", NULL, &hints, &list) == 0);
    int count = 0, v4 = 0, v6 = 0;
    for (struct addrinfo *entry = list; entry != NULL; entry = entry->ai_next) {
        ++count;
        v4 += entry->ai_family == AF_INET;
        v6 += entry->ai_family == AF_INET6;
    }
    CHECK("localhost results", count == 4 && v4 == 2 && v6 == 2 &&
                                   strcmp(list->ai_canonname, "localhost") == 0 &&
                                   list->ai_next->ai_canonname == NULL);
    freeaddrinfo(list);

    hints = (struct addrinfo){.ai_family = AF_INET6, .ai_socktype = SOCK_DGRAM,
                              .ai_flags = AI_PASSIVE};
    CHECK("passive getaddrinfo", getaddrinfo(NULL, "53", &hints, &list) == 0);
    struct sockaddr_in6 any;
    memcpy(&any, list->ai_addr, sizeof(any));
    CHECK("passive result", list->ai_next == NULL && list->ai_protocol == IPPROTO_UDP &&
                                memcmp(&any.sin6_addr, &in6addr_any, sizeof(any.sin6_addr)) == 0 &&
                                any.sin6_port == htons(53));
    freeaddrinfo(list);

    /* Sockets keep their family, so an IPv4 address is no AF_INET6 result. */
    hints = (struct addrinfo){.ai_family = AF_INET6, .ai_flags = AI_V4MAPPED,
                              .ai_socktype = SOCK_STREAM};
    CHECK("no mapped result", getaddrinfo("192.0.2.7", "1", &hints, &list) == EAI_NONAME);

    /* The resolver also takes the address in text. */
    hints = (struct addrinfo){.ai_family = AF_INET, .ai_socktype = SOCK_STREAM};
    CHECK("service name", getaddrinfo("127.0.0.1", "http", &hints, &list) == EAI_SERVICE);
    hints.ai_flags = AI_NUMERICHOST;
    CHECK("numeric host only", getaddrinfo("example.invalid", "1", &hints, &list) == EAI_NONAME);
    hints.ai_flags = 0x8000;
    CHECK("unknown flag", getaddrinfo("127.0.0.1", "1", &hints, &list) == EAI_BADFLAGS);
    hints = (struct addrinfo){.ai_family = 99};
    CHECK("unknown family", getaddrinfo("127.0.0.1", "1", &hints, &list) == EAI_FAMILY);
    hints = (struct addrinfo){.ai_socktype = 99};
    CHECK("unknown type", getaddrinfo("127.0.0.1", "1", &hints, &list) == EAI_SOCKTYPE);
    CHECK("nothing to look up", getaddrinfo(NULL, NULL, NULL, &list) == EAI_NONAME);
    CHECK("message", strcmp(gai_strerror(EAI_NONAME), "Name or service not known") == 0);
    return 0;
}

static int tcp(void) {
    int server = socket(AF_INET, SOCK_STREAM, 0);
    CHECK("server socket", server >= 0);
    CHECK("listen before bind", listen(server, 1) == -1 && errno == EDESTADDRREQ);
    int one = 1;
    CHECK("SO_REUSEADDR", setsockopt(server, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one)) == 0);
    CHECK("unknown option", setsockopt(server, SOL_SOCKET, 999, &one, sizeof(one)) == -1 &&
                                errno == ENOPROTOOPT);
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(0),
                                  .sin_addr.s_addr = htonl(INADDR_ANY)};
    CHECK("port zero", bind(server, (struct sockaddr *)&address, sizeof(address)) == -1 &&
                           errno == EADDRNOTAVAIL);
    /* The wildcard address must accept connections to the loopback address. */
    address.sin_port = htons(TCP_PORT);
    CHECK("bind", bind(server, (struct sockaddr *)&address, sizeof(address)) == 0);
    CHECK("listen", listen(server, SOMAXCONN) == 0);
    CHECK("nonblocking", fcntl(server, F_SETFL, O_NONBLOCK) == 0 &&
                             (fcntl(server, F_GETFL) & O_NONBLOCK));
    CHECK("accept nothing pending", accept(server, NULL, NULL) == -1 && errno == EAGAIN);
    CHECK("blocking", fcntl(server, F_SETFL, 0) == 0);

    int client = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, IPPROTO_TCP);
    CHECK("client socket", client >= 0 && fcntl(client, F_GETFD) == FD_CLOEXEC);
    char buffer[16];
    CHECK("receive unconnected", recv(client, buffer, sizeof(buffer), 0) == -1 &&
                                     errno == ENOTCONN);
    CHECK("TCP_NODELAY", setsockopt(client, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one)) == 0);
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    CHECK("connect", connect(client, (struct sockaddr *)&address, sizeof(address)) == 0);
    CHECK("connect twice", connect(client, (struct sockaddr *)&address, sizeof(address)) == -1 &&
                               errno == EISCONN);

    struct sockaddr_storage peer;
    socklen_t length = sizeof(peer);
    int accepted = accept(server, (struct sockaddr *)&peer, &length);
    CHECK("accept", accepted >= 0);
    struct sockaddr_in peer_in;
    memcpy(&peer_in, &peer, sizeof(peer_in));
    CHECK("accepted peer", length == sizeof(struct sockaddr_in) && peer_in.sin_family == AF_INET &&
                               peer_in.sin_addr.s_addr == htonl(INADDR_LOOPBACK));
    struct sockaddr_in local;
    length = sizeof(local);
    CHECK("getsockname", getsockname(accepted, (struct sockaddr *)&local, &length) == 0 &&
                             local.sin_port == htons(TCP_PORT));
    length = sizeof(local);
    CHECK("getpeername", getpeername(client, (struct sockaddr *)&local, &length) == 0 &&
                             local.sin_port == htons(TCP_PORT));
    /* A truncated address keeps the full length. */
    length = 4;
    CHECK("short getpeername", getpeername(client, (struct sockaddr *)&local, &length) == 0 &&
                                   length == sizeof(local));

    CHECK("send", send(client, "ping", 4, MSG_NOSIGNAL) == 4);
    struct pollfd wait = {.fd = accepted, .events = POLLIN};
    CHECK("poll", poll(&wait, 1, 2000) == 1 && (wait.revents & POLLIN));
    CHECK("recv", recv(accepted, buffer, sizeof(buffer), 0) == 4 && memcmp(buffer, "ping", 4) == 0);
    CHECK("nothing more", recv(accepted, buffer, sizeof(buffer), MSG_DONTWAIT) == -1 &&
                              errno == EAGAIN);
    CHECK("unsupported flag", recv(accepted, buffer, sizeof(buffer), MSG_PEEK) == -1 &&
                                  errno == EOPNOTSUPP);

    /* read and write work on sockets, and duplicates share the connection. */
    int copy = dup(accepted);
    CHECK("dup", copy >= 0 && close(accepted) == 0);
    CHECK("write", write(copy, "pong", 4) == 4);
    CHECK("read", read(client, buffer, sizeof(buffer)) == 4 && memcmp(buffer, "pong", 4) == 0);

    struct stat information;
    CHECK("fstat", fstat(copy, &information) == 0 && S_ISSOCK(information.st_mode));
    CHECK("lseek", lseek(copy, 0, SEEK_SET) == -1 && errno == ESPIPE);
    int type = 0;
    length = sizeof(type);
    CHECK("SO_TYPE", getsockopt(copy, SOL_SOCKET, SO_TYPE, &type, &length) == 0 &&
                         type == SOCK_STREAM);

    CHECK("shutdown", shutdown(client, SHUT_WR) == 0);
    CHECK("send after shutdown", send(client, "x", 1, 0) == -1 && errno == EPIPE);
    CHECK("end of stream", recv(copy, buffer, sizeof(buffer), 0) == 0);
    CHECK("close", close(copy) == 0 && close(client) == 0 && close(server) == 0);
    return 0;
}

static int udp(void) {
    int receiver = socket(AF_INET, SOCK_DGRAM, 0);
    CHECK("receiver socket", receiver >= 0);
    CHECK("listen on a datagram socket", listen(receiver, 1) == -1 && errno == EOPNOTSUPP);
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(UDP_PORT),
                                  .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    CHECK("bind", bind(receiver, (struct sockaddr *)&address, sizeof(address)) == 0);
    CHECK("bind twice", bind(receiver, (struct sockaddr *)&address, sizeof(address)) == -1 &&
                            errno == EINVAL);

    int sender = socket(AF_INET, SOCK_DGRAM, 0);
    CHECK("sender socket", sender >= 0);
    CHECK("no destination", send(sender, "x", 1, 0) == -1 && errno == EDESTADDRREQ);
    /* The first send binds the sender to an ephemeral port. */
    CHECK("sendto", sendto(sender, "datagram", 8, 0, (struct sockaddr *)&address,
                           sizeof(address)) == 8);
    struct sockaddr_in sender_address;
    socklen_t length = sizeof(sender_address);
    CHECK("sender name", getsockname(sender, (struct sockaddr *)&sender_address, &length) == 0 &&
                             ntohs(sender_address.sin_port) >= 49152);

    char buffer[16];
    struct sockaddr_in source;
    length = sizeof(source);
    CHECK("recvfrom", recvfrom(receiver, buffer, sizeof(buffer), 0, (struct sockaddr *)&source,
                               &length) == 8 && memcmp(buffer, "datagram", 8) == 0);
    CHECK("source", length == sizeof(source) && source.sin_port == sender_address.sin_port &&
                        source.sin_addr.s_addr == htonl(INADDR_LOOPBACK));
    CHECK("nothing more", recv(receiver, buffer, sizeof(buffer), MSG_DONTWAIT) == -1 &&
                              errno == EAGAIN);

    /* A datagram larger than the buffer is truncated, and the rest dropped. */
    CHECK("connect", connect(sender, (struct sockaddr *)&address, sizeof(address)) == 0);
    CHECK("send", send(sender, "truncated", 9, 0) == 9);
    CHECK("short recv", recv(receiver, buffer, 5, 0) == 5 && memcmp(buffer, "trunc", 5) == 0);
    CHECK("dropped rest", recv(receiver, buffer, sizeof(buffer), MSG_DONTWAIT) == -1 &&
                              errno == EAGAIN);

    CHECK("reply", sendto(receiver, "ack", 3, 0, (struct sockaddr *)&source, sizeof(source)) == 3);
    CHECK("connected recv", recv(sender, buffer, sizeof(buffer), 0) == 3 &&
                                memcmp(buffer, "ack", 3) == 0);
    CHECK("close", close(sender) == 0 && close(receiver) == 0);
    return 0;
}

static int errors(void) {
    CHECK("unknown family", socket(1, SOCK_STREAM, 0) == -1 && errno == EAFNOSUPPORT);
    CHECK("unknown type", socket(AF_INET, 5, 0) == -1 && errno == EPROTOTYPE);
    CHECK("wrong protocol", socket(AF_INET, SOCK_STREAM, IPPROTO_UDP) == -1 &&
                                errno == EPROTONOSUPPORT);
    CHECK("not a socket", listen(1, 1) == -1 && errno == ENOTSOCK);
    CHECK("bad descriptor", listen(99, 1) == -1 && errno == EBADF);
    int fd = socket(AF_INET6, SOCK_STREAM, 0);
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(1)};
    CHECK("family mismatch", bind(fd, (struct sockaddr *)&address, sizeof(address)) == -1 &&
                                 errno == EAFNOSUPPORT);
    struct sockaddr_in6 address6 = {.sin6_family = AF_INET6, .sin6_port = htons(1)};
    CHECK("short address", bind(fd, (struct sockaddr *)&address6, sizeof(address)) == -1 &&
                               errno == EINVAL);
    CHECK("close", close(fd) == 0);
    /* Nothing listens on that port: the loopback interface resets the connection. */
    fd = socket(AF_INET, SOCK_STREAM, 0);
    address = (struct sockaddr_in){.sin_family = AF_INET, .sin_port = htons(7199),
                                   .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    CHECK("refused", connect(fd, (struct sockaddr *)&address, sizeof(address)) == -1 &&
                         (errno == ECONNREFUSED || errno == ECONNRESET));
    CHECK("close refused", close(fd) == 0);
    return 0;
}

#define FAMILY_TCP_PORT 7111
#define FAMILY_UDP_PORT 7112

/* A wildcard bind keeps its address family. The probe sends IPv4 traffic only, which
   the test's loopback interface carries: an IPv6 wildcard bind must not get it, while an
   IPv4 one gets it and reports IPv4 peers. The other direction runs the same check
   (`same_family` in the ABI, which has a unit test for both). */
static int family(void) {
    struct sockaddr_in6 any6 = {.sin6_family = AF_INET6, .sin6_port = htons(FAMILY_TCP_PORT),
                                .sin6_addr = IN6ADDR_ANY_INIT};
    int server6 = socket(AF_INET6, SOCK_STREAM | SOCK_NONBLOCK, 0);
    CHECK("IPv6 server", server6 >= 0 &&
                             bind(server6, (struct sockaddr *)&any6, sizeof(any6)) == 0 &&
                             listen(server6, 1) == 0);
    /* The IPv4 connection completes its handshake with the listening socket, which
       then refuses it: it is never reported or accepted. */
    int other = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in to4 = {.sin_family = AF_INET, .sin_port = htons(FAMILY_TCP_PORT),
                              .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    CHECK("IPv4 handshake", connect(other, (struct sockaddr *)&to4, sizeof(to4)) == 0);
    struct pollfd wait = {.fd = server6, .events = POLLIN};
    CHECK("poll refuses IPv4", poll(&wait, 1, 100) == 0);
    CHECK("accept refuses IPv4", accept(server6, NULL, NULL) == -1 && errno == EAGAIN);
    CHECK("close IPv6 server", close(other) == 0 && close(server6) == 0);

    struct sockaddr_in any = {.sin_family = AF_INET, .sin_port = htons(FAMILY_TCP_PORT + 2),
                              .sin_addr.s_addr = htonl(INADDR_ANY)};
    int server = socket(AF_INET, SOCK_STREAM, 0);
    CHECK("IPv4 server", server >= 0 && bind(server, (struct sockaddr *)&any, sizeof(any)) == 0 &&
                             listen(server, 1) == 0);
    int client = socket(AF_INET, SOCK_STREAM, 0);
    to4.sin_port = htons(FAMILY_TCP_PORT + 2);
    CHECK("IPv4 connect", connect(client, (struct sockaddr *)&to4, sizeof(to4)) == 0);
    struct sockaddr_storage peer;
    socklen_t length = sizeof(peer);
    int accepted = accept(server, (struct sockaddr *)&peer, &length);
    CHECK("accept IPv4", accepted >= 0 && peer.ss_family == AF_INET &&
                             length == sizeof(struct sockaddr_in));
    CHECK("close TCP", close(accepted) == 0 && close(client) == 0 && close(server) == 0);

    /* UDP: an IPv6 wildcard socket drops IPv4 datagrams. */
    any6.sin6_port = htons(FAMILY_UDP_PORT);
    int receiver6 = socket(AF_INET6, SOCK_DGRAM, 0);
    CHECK("IPv6 receiver", receiver6 >= 0 &&
                               bind(receiver6, (struct sockaddr *)&any6, sizeof(any6)) == 0);
    int sender = socket(AF_INET, SOCK_DGRAM, 0);
    to4.sin_port = htons(FAMILY_UDP_PORT);
    CHECK("send to IPv6 wildcard", sendto(sender, "four", 4, 0, (struct sockaddr *)&to4,
                                          sizeof(to4)) == 4);
    char buffer[8];
    wait = (struct pollfd){.fd = receiver6, .events = POLLIN};
    CHECK("poll drops IPv4", poll(&wait, 1, 100) == 0);
    CHECK("receive drops IPv4", recv(receiver6, buffer, sizeof(buffer), MSG_DONTWAIT) == -1 &&
                                    errno == EAGAIN);
    struct sockaddr_in6 to6 = {.sin6_family = AF_INET6, .sin6_port = htons(FAMILY_UDP_PORT),
                               .sin6_addr = IN6ADDR_LOOPBACK_INIT};
    CHECK("other family destination",
          sendto(sender, "x", 1, 0, (struct sockaddr *)&to6, sizeof(to6)) == -1 &&
              errno == EAFNOSUPPORT);
    CHECK("close IPv6 receiver", close(receiver6) == 0);

    /* An IPv4 wildcard socket gets them, from an IPv4 source. */
    any.sin_port = htons(FAMILY_UDP_PORT + 2);
    int receiver = socket(AF_INET, SOCK_DGRAM, 0);
    CHECK("IPv4 receiver", receiver >= 0 &&
                               bind(receiver, (struct sockaddr *)&any, sizeof(any)) == 0);
    to4.sin_port = htons(FAMILY_UDP_PORT + 2);
    CHECK("send to IPv4 wildcard", sendto(sender, "four", 4, 0, (struct sockaddr *)&to4,
                                          sizeof(to4)) == 4);
    length = sizeof(peer);
    CHECK("receive IPv4",
          recvfrom(receiver, buffer, sizeof(buffer), 0, (struct sockaddr *)&peer, &length) == 4 &&
              memcmp(buffer, "four", 4) == 0 && peer.ss_family == AF_INET &&
              length == sizeof(struct sockaddr_in));

    /* An AF_INET6 socket does not reach IPv4 through a mapped address. */
    struct sockaddr_in6 mapped = {.sin6_family = AF_INET6, .sin6_port = htons(FAMILY_TCP_PORT)};
    mapped.sin6_addr.s6_addr[10] = 0xff;
    mapped.sin6_addr.s6_addr[11] = 0xff;
    mapped.sin6_addr.s6_addr[12] = 127;
    mapped.sin6_addr.s6_addr[15] = 1;
    int six = socket(AF_INET6, SOCK_STREAM, 0);
    CHECK("mapped address", connect(six, (struct sockaddr *)&mapped, sizeof(mapped)) == -1 &&
                                errno == ENETUNREACH);
    CHECK("close UDP", close(six) == 0 && close(sender) == 0 && close(receiver) == 0);
    puts("family: ok");
    return 0;
}

static int sockets(void) {
    if (address_text() || resolution() || tcp() || udp() || errors())
        return 1;
    puts("sockets: ok");
    return 0;
}

static int no_network(void) {
    CHECK("socket", socket(AF_INET, SOCK_STREAM, 0) == -1 && errno == EACCES);
    struct addrinfo *list = NULL;
    struct addrinfo hints = {.ai_family = AF_INET, .ai_socktype = SOCK_STREAM};
    CHECK("numeric getaddrinfo", getaddrinfo("127.0.0.1", "1", &hints, &list) == 0);
    freeaddrinfo(list);
    CHECK("resolver", getaddrinfo("example.invalid", "1", &hints, &list) == EAI_SYSTEM &&
                          errno == EACCES);
    puts("no-network: ok");
    return 0;
}

/* The startup block, kept by the xila_main wrapper for the raw spawn calls. */
static const XilaStartup *startup;

int32_t __real_xila_main(const XilaStartup *start);
int32_t __wrap_xila_main(const XilaStartup *start) {
    startup = start;
    return __real_xila_main(start);
}

/* Spawn this program as `mode` with the network rights in `flags`. Returns the status of
   xila_process_spawn and, when it succeeded, the child's exit status in `status`. */
static XilaStatus spawn_child(const char *mode, uint32_t flags, int32_t *status) {
    XilaString arguments[] = {{.data = mode, .length = strlen(mode)}};
    XilaSpawn request = {
        .size = sizeof(request),
        .flags = flags,
        .arguments = arguments,
        .argument_count = 1,
        .standard_input = startup->standard_input,
        .standard_output = startup->standard_output,
        .standard_error = startup->standard_error,
    };
    XilaChild *child = NULL;
    XilaStatus result =
        xila_process_spawn(startup->process, startup->home, "network", 7, &request, &child);
    if (result != XILA_STATUS_OK)
        return result;
    result = xila_process_wait(startup->process, child, 0, status);
    (void)xila_process_child_close(child);
    return result;
}

/* Launched with connect and resolve only. */
static int spawn(void) {
    CHECK("granted", startup->network != NULL);
    int32_t status = -1;
    CHECK("widen to listen", spawn_child("child", XILA_SPAWN_NETWORK_LISTEN, &status) ==
                                 XILA_STATUS_PERMISSION_DENIED);
    CHECK("widen with a subset",
          spawn_child("child", XILA_SPAWN_NETWORK_CONNECT | XILA_SPAWN_NETWORK_LISTEN, &status) ==
              XILA_STATUS_PERMISSION_DENIED);
    CHECK("unknown flag", spawn_child("child", 8, &status) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK("narrow to connect",
          spawn_child("child", XILA_SPAWN_NETWORK_CONNECT, &status) == XILA_STATUS_OK &&
              status == 0);
    CHECK("inherit nothing",
          spawn_child("no-network", 0, &status) == XILA_STATUS_OK && status == 0);
    puts("spawn: ok");
    return 0;
}

/* Spawned with connect only: it can create sockets but neither listen nor resolve. */
static int child(void) {
    CHECK("child granted", startup->network != NULL);
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    CHECK("child socket", fd >= 0);
    struct sockaddr_in address = {.sin_family = AF_INET, .sin_port = htons(7121),
                                  .sin_addr.s_addr = htonl(INADDR_LOOPBACK)};
    CHECK("child bind", bind(fd, (struct sockaddr *)&address, sizeof(address)) == 0);
    CHECK("child listen", listen(fd, 1) == -1 && errno == EACCES);
    struct addrinfo hints = {.ai_family = AF_INET, .ai_socktype = SOCK_STREAM};
    struct addrinfo *list = NULL;
    CHECK("child resolve", getaddrinfo("example.invalid", "1", &hints, &list) == EAI_SYSTEM &&
                               errno == EACCES);
    CHECK("child close", close(fd) == 0);
    return 0;
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    if (strcmp(mode, "sockets") == 0)
        return sockets();
    if (strcmp(mode, "family") == 0)
        return family();
    if (strcmp(mode, "no-network") == 0)
        return no_network();
    if (strcmp(mode, "spawn") == 0)
        return spawn();
    if (strcmp(mode, "child") == 0)
        return child();
    fprintf(stderr, "network: unknown scenario \"%s\"\n", mode);
    return 2;
}
