#ifndef XILA_APPLICATION_H
#define XILA_APPLICATION_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define XILA_ABI_VERSION_MAJOR 0

#define XILA_ABI_VERSION_MINOR 1

#define XILA_RIGHT_READ 1

#define XILA_RIGHT_WRITE 2

/**
 * Allow removing directories as well as files. Removal never follows the final link.
 */
#define XILA_REMOVE_DIRECTORY 1

#define XILA_OPEN_READ 1

#define XILA_OPEN_WRITE 2

#define XILA_OPEN_CREATE 4

#define XILA_OPEN_EXCLUSIVE 8

#define XILA_OPEN_TRUNCATE 16

#define XILA_OPEN_APPEND 32

#define XILA_OPEN_NON_BLOCKING 64

#define XILA_SEEK_START 0

#define XILA_SEEK_CURRENT 1

#define XILA_SEEK_END 2

#define XILA_STATISTICS_NO_FOLLOW 1

#define XILA_KIND_FILE 1

#define XILA_KIND_DIRECTORY 2

#define XILA_KIND_BLOCK_DEVICE 3

#define XILA_KIND_CHARACTER_DEVICE 4

#define XILA_KIND_PIPE 5

#define XILA_KIND_SOCKET 6

#define XILA_KIND_SYMLINK 7

#define XILA_NETWORK_CONNECT 1

#define XILA_NETWORK_LISTEN 2

#define XILA_NETWORK_RESOLVE 4

#define XILA_ADDRESS_IPV4 4

#define XILA_ADDRESS_IPV6 6

#define XILA_SHUTDOWN_WRITE 1

#if defined(__arm__)
/**
 * Alignment guaranteed for every allocation, including zero-length requests.
 */
#define XILA_HEAP_ALIGNMENT 8
#endif

#if !defined(__arm__)
#define XILA_HEAP_ALIGNMENT 16
#endif

/**
 * Native status values mirror the unified error kinds one-to-one. Zero is success.
 */
enum XilaStatus
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : uint32_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
  XILA_STATUS_OK = 0,
  XILA_STATUS_INVALID_ARGUMENT = 257,
  XILA_STATUS_INVALID_DATA = 258,
  XILA_STATUS_PERMISSION_DENIED = 259,
  XILA_STATUS_OPERATION_NOT_PERMITTED = 260,
  XILA_STATUS_INPUT_OUTPUT = 261,
  XILA_STATUS_OUT_OF_MEMORY = 262,
  XILA_STATUS_WOULD_BLOCK = 263,
  XILA_STATUS_RESOURCE_BUSY = 264,
  XILA_STATUS_INTERRUPTED = 265,
  XILA_STATUS_TIMED_OUT = 266,
  XILA_STATUS_UNSUPPORTED = 267,
  XILA_STATUS_OVERFLOW = 268,
  XILA_STATUS_WRITE_ZERO = 269,
  XILA_STATUS_NO_BUFFERS = 270,
  XILA_STATUS_NOT_FOUND = 271,
  XILA_STATUS_ALREADY_EXISTS = 272,
  XILA_STATUS_INVALID_FLAGS = 273,
  XILA_STATUS_INVALID_STATE = 274,
  XILA_STATUS_NOT_IMPLEMENTED = 275,
  XILA_STATUS_EXEC_FORMAT = 276,
  XILA_STATUS_NO_CHILD = 277,
  XILA_STATUS_ARGUMENT_LIST_TOO_LONG = 278,
  XILA_STATUS_DEADLOCK = 279,
  XILA_STATUS_BAD_DESCRIPTOR = 513,
  XILA_STATUS_INVALID_PATH = 514,
  XILA_STATUS_MISSING_PATH_COMPONENT = 515,
  XILA_STATUS_MISSING_FINAL_ENTRY = 516,
  XILA_STATUS_NAME_TOO_LONG = 517,
  XILA_STATUS_SYMLINK_LOOP = 518,
  XILA_STATUS_NOT_A_DIRECTORY = 519,
  XILA_STATUS_IS_A_DIRECTORY = 520,
  XILA_STATUS_DIRECTORY_NOT_EMPTY = 521,
  XILA_STATUS_NO_SPACE = 522,
  XILA_STATUS_FILE_TOO_LARGE = 523,
  XILA_STATUS_PROCESS_FILE_LIMIT = 524,
  XILA_STATUS_SYSTEM_FILE_LIMIT = 525,
  XILA_STATUS_CROSS_DEVICE = 526,
  XILA_STATUS_PATH_ESCAPES_ROOT = 527,
  XILA_STATUS_CORRUPT_DATA = 528,
  XILA_STATUS_MISSING_ATTRIBUTE = 529,
  XILA_STATUS_STALE_HANDLE = 530,
  XILA_STATUS_READ_ONLY_FILE_SYSTEM = 531,
  XILA_STATUS_INVALID_SEEK = 532,
  XILA_STATUS_UNAVAILABLE = 769,
  XILA_STATUS_NO_SUCH_TASK = 770,
  XILA_STATUS_INAPPROPRIATE_IO_CONTROL = 771,
  XILA_STATUS_CONNECTION_REFUSED = 1025,
  XILA_STATUS_CONNECTION_RESET = 1026,
  XILA_STATUS_CONNECTION_ABORTED = 1027,
  XILA_STATUS_NOT_CONNECTED = 1028,
  XILA_STATUS_ALREADY_CONNECTED = 1029,
  XILA_STATUS_ADDRESS_IN_USE = 1030,
  XILA_STATUS_ADDRESS_NOT_AVAILABLE = 1031,
  XILA_STATUS_DESTINATION_REQUIRED = 1032,
  XILA_STATUS_HOST_UNREACHABLE = 1033,
  XILA_STATUS_NETWORK_UNREACHABLE = 1034,
  XILA_STATUS_NETWORK_DOWN = 1035,
  XILA_STATUS_BROKEN_PIPE = 1036,
  XILA_STATUS_MESSAGE_TOO_LARGE = 1037,
  XILA_STATUS_DATAGRAM_TRUNCATED = 1038,
  XILA_STATUS_SOCKET_OPERATION_UNSUPPORTED = 1039,
  XILA_STATUS_PROTOCOL_UNSUPPORTED = 1040,
  XILA_STATUS_ADDRESS_FAMILY_UNSUPPORTED = 1041,
  XILA_STATUS_IN_PROGRESS = 1042,
  XILA_STATUS_ALREADY_IN_PROGRESS = 1043,
  XILA_STATUS_PROTOCOL_ERROR = 1044,
  XILA_STATUS_NOT_A_SOCKET = 1045,
  XILA_STATUS_INVALID_DNS_NAME = 1281,
  XILA_STATUS_UNSUPPORTED_DNS_QUERY = 1282,
  XILA_STATUS_NAME_NOT_RESOLVED = 1283,
  XILA_STATUS_RESOLVER_TEMPORARY_FAILURE = 1284,
  XILA_STATUS_RESOLVER_FAILURE = 1285,
  XILA_STATUS_END = 1793,
  XILA_STATUS_BUFFER_TOO_SMALL = 1794,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum XilaStatus XilaStatus;
#else
typedef uint32_t XilaStatus;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

/**
 * Opaque path capability. Native callers must serialize access and preserve ownership.
 */
typedef struct XilaDirectory XilaDirectory;

/**
 * Opaque owned listing with at most one pending VFS entry for buffer retries.
 */
typedef struct XilaDirectoryIterator XilaDirectoryIterator;

/**
 * Opaque owned file. No handle lookup or buffer allocation is required for I/O.
 */
typedef struct XilaFile XilaFile;

/**
 * Opaque native heap capability. The process owner drops it after all native
 * execution ends. Keep ownership outside any stack that can be abandoned.
 */
typedef struct XilaHeap XilaHeap;

/**
 * Host-granted network authority. Future host/port restrictions belong here.
 */
typedef struct XilaNetwork XilaNetwork;

/**
 * Owned TCP connection. Its creation grants bidirectional I/O authority.
 */
typedef struct XilaTcp XilaTcp;

/**
 * One pending connection, with no additional backlog. Accept replenishes the socket.
 */
typedef struct XilaTcpListener XilaTcpListener;

/**
 * Owned UDP socket. Sending additionally requires CONNECT on its parent capability.
 */
typedef struct XilaUdp XilaUdp;

/**
 * UTF-8 bytes, not NUL-terminated. A null data pointer is valid only at length zero.
 */
typedef struct XilaString {
  const char *data;
  size_t length;
} XilaString;

/**
 * Versioned output: initialize size to sizeof(XilaStatistics) before each call.
 */
typedef struct XilaStatistics {
  uint32_t size;
  uint32_t kind;
  uint32_t permissions;
  uint32_t owner_user;
  uint32_t owner_group;
  uint32_t reserved;
  uint64_t file_size;
  uint64_t link_count;
  uint64_t inode;
  int64_t access_nanoseconds;
  int64_t modification_nanoseconds;
  int64_t status_change_nanoseconds;
} XilaStatistics;

/**
 * Fixed-width address. Bytes are in network order; integer fields are host order.
 * IPv4 uses the first four bytes and requires the remaining bytes to be zero.
 * Scope must be zero: Core does not support scoped IPv6 endpoints.
 */
typedef struct XilaNetworkAddress {
  uint32_t family;
  uint8_t bytes[16];
  uint16_t port;
  uint16_t reserved;
  uint32_t scope_id;
} XilaNetworkAddress;

/**
 * Native application entry points, in slot order. The header precedes slot zero.
 * Networking slots return Unsupported when the runtime omits networking.
 */
typedef struct XilaApi {
  /**
   * Packed major and minor ABI version.
   */
  uint32_t version;
  /**
   * Total size in bytes, including this header.
   */
  uint32_t size;
  XilaStatus (*directory_open)(const struct XilaDirectory *parent,
                               const char *path,
                               size_t path_length,
                               uint32_t rights,
                               struct XilaDirectory **directory);
  XilaStatus (*directory_close)(struct XilaDirectory *directory);
  XilaStatus (*directory_create)(const struct XilaDirectory *directory,
                                 const char *path,
                                 size_t length);
  XilaStatus (*directory_remove)(const struct XilaDirectory *directory,
                                 const char *path,
                                 size_t length,
                                 uint32_t flags);
  XilaStatus (*directory_rename)(const struct XilaDirectory *from,
                                 const char *from_path,
                                 size_t from_length,
                                 const struct XilaDirectory *to,
                                 const char *to_path,
                                 size_t to_length);
  XilaStatus (*file_open)(const struct XilaDirectory *directory,
                          const char *path,
                          size_t path_length,
                          uint32_t flags,
                          struct XilaFile **file);
  XilaStatus (*file_read)(struct XilaFile *file, void *buffer, size_t size, size_t *count);
  XilaStatus (*file_write)(struct XilaFile *file, const void *buffer, size_t size, size_t *count);
  XilaStatus (*file_seek)(struct XilaFile *file, int64_t offset, uint32_t whence, uint64_t *position);
  XilaStatus (*file_flush)(struct XilaFile *file);
  XilaStatus (*file_close)(struct XilaFile *file);
  XilaStatus (*file_set_size)(struct XilaFile *file, uint64_t size);
  XilaStatus (*file_get_statistics)(struct XilaFile *file, struct XilaStatistics *output);
  XilaStatus (*directory_get_statistics)(const struct XilaDirectory *directory,
                                         const char *path,
                                         size_t length,
                                         uint32_t flags,
                                         struct XilaStatistics *output);
  XilaStatus (*directory_iterator_open)(const struct XilaDirectory *directory,
                                        const char *path,
                                        size_t length,
                                        struct XilaDirectoryIterator **output);
  XilaStatus (*directory_iterator_next)(struct XilaDirectoryIterator *iterator,
                                        char *name_buffer,
                                        size_t capacity,
                                        size_t *name_length,
                                        uint32_t *kind);
  XilaStatus (*directory_iterator_rewind)(struct XilaDirectoryIterator *iterator);
  XilaStatus (*directory_iterator_close)(struct XilaDirectoryIterator *iterator);
  XilaStatus (*time_monotonic)(uint64_t *nanoseconds);
  XilaStatus (*time_realtime)(int64_t *nanoseconds);
  XilaStatus (*network_close)(struct XilaNetwork *network);
  XilaStatus (*tcp_connect)(const struct XilaNetwork *network,
                            const struct XilaNetworkAddress *address,
                            uint32_t timeout_ms,
                            struct XilaTcp **connection);
  XilaStatus (*tcp_listen)(const struct XilaNetwork *network,
                           const struct XilaNetworkAddress *address,
                           struct XilaTcpListener **listener);
  XilaStatus (*tcp_accept)(struct XilaTcpListener *listener, struct XilaTcp **connection);
  XilaStatus (*tcp_close)(struct XilaTcp *socket);
  XilaStatus (*tcp_listener_close)(struct XilaTcpListener *socket);
  XilaStatus (*tcp_shutdown)(struct XilaTcp *socket, uint32_t direction);
  XilaStatus (*tcp_send)(struct XilaTcp *socket, const void *buffer, size_t size, size_t *count);
  XilaStatus (*tcp_receive)(struct XilaTcp *socket, void *buffer, size_t size, size_t *count);
  XilaStatus (*tcp_local_address)(const struct XilaTcp *socket, struct XilaNetworkAddress *address);
  XilaStatus (*tcp_peer_address)(const struct XilaTcp *socket, struct XilaNetworkAddress *address);
  XilaStatus (*udp_bind)(const struct XilaNetwork *network,
                         const struct XilaNetworkAddress *address,
                         struct XilaUdp **socket);
  XilaStatus (*udp_close)(struct XilaUdp *socket);
  XilaStatus (*udp_send_to)(struct XilaUdp *socket,
                            const void *buffer,
                            size_t size,
                            const struct XilaNetworkAddress *address,
                            size_t *count);
  XilaStatus (*udp_receive_from)(struct XilaUdp *socket,
                                 void *buffer,
                                 size_t size,
                                 struct XilaNetworkAddress *address,
                                 size_t *count);
  XilaStatus (*network_resolve)(const struct XilaNetwork *network,
                                const char *host,
                                size_t host_length,
                                uint32_t family,
                                struct XilaNetworkAddress *addresses,
                                size_t *count);
  /**
   * Allocate uninitialized memory aligned to XILA_HEAP_ALIGNMENT.
   */
  XilaStatus (*heap_allocate)(struct XilaHeap *heap, size_t size, void **allocation);
  /**
   * Resize an allocation. Zero size frees the block and stores null.
   */
  XilaStatus (*heap_reallocate)(struct XilaHeap *heap, void *pointer, size_t size, void **allocation);
  /**
   * Release an allocation from this heap. Null succeeds.
   */
  XilaStatus (*heap_free)(struct XilaHeap *heap, void *pointer);
} XilaApi;

/**
 * Host-owned startup data, valid only for the duration of the entry call.
 * The API is always present. Capability pointers may be null when not granted.
 * A null argument array has argument_count zero.
 * Applications must not close, retain or transfer these objects; closing them is
 * undefined behavior. Objects opened by the application must still be closed.
 */
typedef struct XilaStartup {
  uint32_t size;
  const struct XilaApi *api;
  struct XilaHeap *heap;
  const struct XilaString *arguments;
  size_t argument_count;
  struct XilaFile *standard_input;
  struct XilaFile *standard_output;
  struct XilaFile *standard_error;
  struct XilaDirectory *home;
} XilaStartup;

#endif  /* XILA_APPLICATION_H */
