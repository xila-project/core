/* getaddrinfo over the host's resolver (xila_network_resolve).
 *
 * There is no hosts file and no services database: a service is a decimal port, and
 * "localhost" is the only name answered without the resolver. Every result is one heap
 * block holding the addrinfo and its address; the first one also holds the canonical
 * name, which is the node as given. */
#include <arpa/inet.h>
#include <netdb.h>
#include <string.h>
#include <strings.h>

#include "internal.h"

#define SUPPORTED_FLAGS                                                                        \
    (AI_PASSIVE | AI_CANONNAME | AI_NUMERICHOST | AI_V4MAPPED | AI_ALL | AI_ADDRCONFIG |      \
     AI_NUMERICSERV)

/* Addresses asked for at once before the resolver is asked how many there are. */
#define RESOLVE_BATCH 4

struct entry {
    struct addrinfo information;
    union {
        struct sockaddr_in in;
        struct sockaddr_in6 in6;
    } address;
};

struct list {
    XilaNetworkAddress *addresses;
    size_t count;
    size_t capacity;
};

static int append(struct list *list, const XilaNetworkAddress *addresses, size_t count) {
    if (list->count + count > list->capacity) {
        size_t capacity = list->capacity * 2 + count;
        XilaNetworkAddress *grown = xila_heap_alloc(capacity * sizeof(*grown));
        if (grown == NULL)
            return EAI_MEMORY;
        if (list->count != 0)
            memcpy(grown, list->addresses, list->count * sizeof(*grown));
        xila_heap_release(list->addresses);
        list->addresses = grown;
        list->capacity = capacity;
    }
    memcpy(list->addresses + list->count, addresses, count * sizeof(*addresses));
    list->count += count;
    return 0;
}

static int resolver_error(XilaStatus status) {
    switch (status) {
    case XILA_STATUS_NAME_NOT_RESOLVED:
    case XILA_STATUS_NOT_FOUND:
    case XILA_STATUS_INVALID_DNS_NAME:
        return EAI_NONAME;
    case XILA_STATUS_RESOLVER_TEMPORARY_FAILURE:
    case XILA_STATUS_TIMED_OUT:
    case XILA_STATUS_WOULD_BLOCK:
        return EAI_AGAIN;
    case XILA_STATUS_OUT_OF_MEMORY:
        return EAI_MEMORY;
    case XILA_STATUS_RESOLVER_FAILURE:
        return EAI_FAIL;
    default:
        errno = xila_errno_from_status(status);
        return EAI_SYSTEM;
    }
}

/* Append the addresses of one family. Returns 0 or an EAI_ value. */
static int resolve(const char *node, uint32_t family, struct list *list) {
    const XilaNetwork *network = xila_process.startup->network;
    if (network == NULL) {
        errno = EACCES; /* the host granted no network capability */
        return EAI_SYSTEM;
    }
    XilaNetworkAddress batch[RESOLVE_BATCH];
    size_t count = RESOLVE_BATCH;
    XilaStatus status = xila_network_resolve(network, node, strlen(node), family, batch, &count);
    if (status == XILA_STATUS_OK)
        return count == 0 ? EAI_NONAME : append(list, batch, count);
    /* More than a batch: ask how many, then fetch them all. The answer can change in
       between, so try a few times. */
    for (int attempt = 0; status == XILA_STATUS_OVERFLOW && attempt < 3; ++attempt) {
        size_t needed = 0;
        status = xila_network_resolve(network, node, strlen(node), family, NULL, &needed);
        if (status != XILA_STATUS_OK)
            break;
        if (needed == 0)
            return EAI_NONAME;
        XilaNetworkAddress *all = xila_heap_alloc(needed * sizeof(*all));
        if (all == NULL)
            return EAI_MEMORY;
        count = needed;
        status = xila_network_resolve(network, node, strlen(node), family, all, &count);
        int error = status == XILA_STATUS_OK ? append(list, all, count) : 0;
        xila_heap_release(all);
        if (status == XILA_STATUS_OK)
            return error;
    }
    return resolver_error(status);
}

static int parse_port(const char *service, int flags, uint16_t *port) {
    if (service == NULL || service[0] == '\0') {
        *port = 0;
        return service == NULL ? 0 : EAI_SERVICE;
    }
    unsigned long value = 0;
    for (const char *cursor = service; *cursor != '\0'; ++cursor) {
        if (*cursor < '0' || *cursor > '9' || value > 65535)
            return (flags & AI_NUMERICSERV) ? EAI_NONAME : EAI_SERVICE;
        value = value * 10 + (unsigned long)(*cursor - '0');
    }
    if (value > 65535)
        return EAI_SERVICE;
    *port = (uint16_t)value;
    return 0;
}

static XilaNetworkAddress address_of(uint32_t family, const void *bytes) {
    XilaNetworkAddress address = {.family = family};
    memcpy(address.bytes, bytes, family == XILA_ADDRESS_IPV4 ? 4 : 16);
    return address;
}

/* Collect the addresses of `node` for the requested family. */
static int lookup(const char *node, int family, int flags, struct list *list) {
    int want_v4 = family != AF_INET6, want_v6 = family != AF_INET;
    static const uint8_t any[16] = {0};
    static const uint8_t loopback_v4[4] = {127, 0, 0, 1};
    static const uint8_t loopback_v6[16] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1};
    uint8_t bytes[16];

    int local = node == NULL || strcasecmp(node, "localhost") == 0;
    if (local) {
        int passive = node == NULL && (flags & AI_PASSIVE);
        XilaNetworkAddress addresses[2];
        size_t count = 0;
        if (want_v4)
            addresses[count++] = address_of(XILA_ADDRESS_IPV4, passive ? any : loopback_v4);
        if (want_v6)
            addresses[count++] = address_of(XILA_ADDRESS_IPV6, passive ? any : loopback_v6);
        return append(list, addresses, count);
    }
    if (inet_pton(AF_INET, node, bytes) == 1) {
        if (!want_v4 && !(flags & AI_V4MAPPED))
            return EAI_NONAME;
        XilaNetworkAddress address = address_of(XILA_ADDRESS_IPV4, bytes);
        return append(list, &address, 1);
    }
    if (inet_pton(AF_INET6, node, bytes) == 1) {
        if (!want_v6)
            return EAI_NONAME;
        XilaNetworkAddress address = address_of(XILA_ADDRESS_IPV6, bytes);
        return append(list, &address, 1);
    }
    if (flags & AI_NUMERICHOST)
        return EAI_NONAME;

    int v6_error = 0, v4_error = 0;
    if (want_v6)
        v6_error = resolve(node, XILA_ADDRESS_IPV6, list);
    size_t v6_count = list->count;
    /* An AF_INET6 lookup gets IPv4 results, mapped, with AI_V4MAPPED when there is no
       IPv6 address, or always with AI_ALL. */
    int mapped = family == AF_INET6 && (flags & AI_V4MAPPED) &&
                 (v6_count == 0 || (flags & AI_ALL));
    if (want_v4 || mapped)
        v4_error = resolve(node, XILA_ADDRESS_IPV4, list);
    if (list->count != 0)
        return 0;
    /* Report a temporary failure before a missing name: retrying can help. */
    if (v6_error == EAI_AGAIN || v4_error == EAI_AGAIN)
        return EAI_AGAIN;
    return v4_error != 0 ? v4_error : (v6_error != 0 ? v6_error : EAI_NONAME);
}

/* Build one result. Returns NULL when memory runs out. */
static struct addrinfo *build(const XilaNetworkAddress *native, int family, int socktype,
                              uint16_t port, const char *canonical) {
    size_t name_length = canonical != NULL ? strlen(canonical) + 1 : 0;
    struct entry *entry = xila_heap_alloc(sizeof(*entry) + name_length);
    if (entry == NULL)
        return NULL;
    memset(entry, 0, sizeof(*entry));
    /* An IPv4 address in an AF_INET6 result is IPv4-mapped. */
    int result_family = family == AF_INET6 || native->family == XILA_ADDRESS_IPV6 ? AF_INET6 : AF_INET;
    if (result_family == AF_INET) {
        entry->address.in.sin_family = AF_INET;
        entry->address.in.sin_port = htons(port);
        memcpy(&entry->address.in.sin_addr, native->bytes, 4);
        entry->information.ai_addrlen = sizeof(entry->address.in);
    } else {
        entry->address.in6.sin6_family = AF_INET6;
        entry->address.in6.sin6_port = htons(port);
        if (native->family == XILA_ADDRESS_IPV4) {
            entry->address.in6.sin6_addr.s6_addr[10] = 0xff;
            entry->address.in6.sin6_addr.s6_addr[11] = 0xff;
            memcpy(entry->address.in6.sin6_addr.s6_addr + 12, native->bytes, 4);
        } else {
            memcpy(entry->address.in6.sin6_addr.s6_addr, native->bytes, 16);
        }
        entry->information.ai_addrlen = sizeof(entry->address.in6);
    }
    entry->information.ai_family = result_family;
    entry->information.ai_socktype = socktype;
    entry->information.ai_protocol = socktype == SOCK_STREAM ? IPPROTO_TCP : IPPROTO_UDP;
    entry->information.ai_addr = (struct sockaddr *)&entry->address;
    if (canonical != NULL) {
        entry->information.ai_canonname = (char *)(entry + 1);
        memcpy(entry->information.ai_canonname, canonical, name_length);
    }
    return &entry->information;
}

int getaddrinfo(const char *restrict node, const char *restrict service,
                const struct addrinfo *restrict hints, struct addrinfo **restrict result) {
    int flags = hints != NULL ? hints->ai_flags : AI_V4MAPPED | AI_ADDRCONFIG;
    int family = hints != NULL ? hints->ai_family : AF_UNSPEC;
    int socktype = hints != NULL ? hints->ai_socktype : 0;
    int protocol = hints != NULL ? hints->ai_protocol : 0;
    if (result == NULL) {
        errno = EFAULT;
        return EAI_SYSTEM;
    }
    if (flags & ~SUPPORTED_FLAGS)
        return EAI_BADFLAGS;
    if (node == NULL && service == NULL)
        return EAI_NONAME;
    if ((flags & AI_CANONNAME) && node == NULL)
        return EAI_BADFLAGS;
    if (family != AF_UNSPEC && family != AF_INET && family != AF_INET6)
        return EAI_FAMILY;
    if (socktype != 0 && socktype != SOCK_STREAM && socktype != SOCK_DGRAM)
        return EAI_SOCKTYPE;
    if (protocol == IPPROTO_TCP && socktype == 0)
        socktype = SOCK_STREAM;
    else if (protocol == IPPROTO_UDP && socktype == 0)
        socktype = SOCK_DGRAM;
    else if (protocol != 0 &&
             protocol != (socktype == SOCK_STREAM ? IPPROTO_TCP : IPPROTO_UDP))
        return EAI_SOCKTYPE;

    uint16_t port = 0;
    int error = parse_port(service, flags, &port);
    if (error != 0)
        return error;

    struct list list = {0};
    error = lookup(node, family, flags, &list);
    struct addrinfo *head = NULL, **tail = &head;
    static const int both[] = {SOCK_STREAM, SOCK_DGRAM};
    const int *types = socktype != 0 ? &socktype : both;
    size_t type_count = socktype != 0 ? 1 : 2;
    for (size_t index = 0; error == 0 && index < list.count; ++index) {
        for (size_t type = 0; type < type_count; ++type) {
            const char *canonical = head == NULL && (flags & AI_CANONNAME) ? node : NULL;
            struct addrinfo *built = build(&list.addresses[index], family, types[type], port,
                                           canonical);
            if (built == NULL) {
                error = EAI_MEMORY;
                break;
            }
            *tail = built;
            tail = &built->ai_next;
        }
    }
    xila_heap_release(list.addresses);
    if (error != 0) {
        freeaddrinfo(head);
        return error;
    }
    *result = head;
    return 0;
}

void freeaddrinfo(struct addrinfo *list) {
    while (list != NULL) {
        struct addrinfo *next = list->ai_next;
        xila_heap_release(list); /* the addrinfo is the start of its block */
        list = next;
    }
}

const char *gai_strerror(int error) {
    switch (error) {
    case 0:
        return "Success";
    case EAI_BADFLAGS:
        return "Invalid flags";
    case EAI_NONAME:
        return "Name or service not known";
    case EAI_AGAIN:
        return "Temporary failure in name resolution";
    case EAI_FAIL:
        return "Non-recoverable failure in name resolution";
    case EAI_FAMILY:
        return "Address family not supported";
    case EAI_SOCKTYPE:
        return "Socket type not supported";
    case EAI_SERVICE:
        return "Service not supported for socket type";
    case EAI_MEMORY:
        return "Memory allocation failure";
    case EAI_SYSTEM:
        return "System error";
    case EAI_OVERFLOW:
        return "Argument buffer overflow";
    default:
        return "Unknown error";
    }
}
