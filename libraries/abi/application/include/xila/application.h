#ifndef XILA_APPLICATION_H
#define XILA_APPLICATION_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

/**
 * Major version of the Xila release this ABI belongs to. Native images
 * record `(XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR` in their ABI note.
 */
#define XILA_VERSION_MAJOR 0

/**
 * Minor version of the Xila release this ABI belongs to.
 */
#define XILA_VERSION_MINOR 1

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

#define XILA_CONTROL_BYTES_AVAILABLE 1

#define XILA_CONTROL_GET_FILE_FLAGS 2

#define XILA_CONTROL_SET_FILE_FLAGS 3

#define XILA_PIPE_CAPACITY 512

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

#define XILA_POLL_IGNORE 0

#define XILA_POLL_FILE 1

#define XILA_POLL_DIRECTORY 2

#define XILA_POLL_TCP 3

#define XILA_POLL_TCP_LISTENER 4

#define XILA_POLL_UDP 5

#define XILA_POLL_IN 1

#define XILA_POLL_OUT 2

#define XILA_POLL_ERR 4

#define XILA_POLL_HUP 8

#define XILA_POLL_NVAL 16

/**
 * `xila_process_wait` flag: return `WouldBlock` instead of waiting for a running child.
 */
#define XILA_WAIT_NO_HANG 1

#define XILA_THREAD_STACK_MIN 2048

#define XILA_THREAD_STACK_MAX (64 * 1024)

#define XILA_THREAD_KEYS_MAX 8

#define XILA_MUTEX_RECURSIVE 1

#define XILA_MUTEX_TRY 1

/**
 * Monotonic clock identifier for `xila_time_resolution`.
 */
#define XILA_CLOCK_MONOTONIC 0

/**
 * Realtime (Unix UTC) clock identifier for `xila_time_resolution`.
 */
#define XILA_CLOCK_REALTIME 1

#if defined(__arm__)
/**
 * Alignment guaranteed for every allocation, including zero-length requests.
 */
#define XILA_HEAP_ALIGNMENT 8
#endif

#if !defined(__arm__)
/**
 * Alignment guaranteed for every allocation, including zero-length requests.
 */
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
 * A spawned child, owned by the process that spawned it. It holds the child's
 * exit status until the owner waits for it, so a child that finished and was not
 * waited for costs one signal slot and no task. `xila_process_child_close`
 * releases it, waited for or not.
 */
typedef struct XilaChild XilaChild;

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
 *
 * Native applications are trusted, so misuse detection is cheap rather than
 * hardened. Every block starts with a header whose tag is the heap's key XOR
 * the block address. Free and resize read the header that would precede the
 * pointer and reject it as `InvalidArgument` unless it is aligned and its tag
 * matches: an interior pointer or a double free (including one through a
 * pointer that a moving resize left stale) is caught, and so is a
 * foreign-heap pointer unless the two heaps' keys collide, which is
 * unlikely. Like a C allocator, this reads through the pointer, so it needs
 * readable memory below it. It cannot detect a freed block whose memory was
 * reused by a later allocation of this heap, which then identifies the new
 * block. Metadata lives in the block, so no operation allocates it and a
 * shrinking resize never fails because of it.
 */
typedef struct XilaHeap XilaHeap;

/**
 * Host-granted network authority. Future host/port restrictions belong here.
 */
typedef struct XilaNetwork XilaNetwork;

/**
 * Opaque capability of the running process: its identity, environment, children and
 * exit. The host grants it through `XilaStartup.process`; the pointer is borrowed for
 * the entry call and a process must not pass it to another process.
 */
typedef struct XilaProcess XilaProcess;

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
 * Arguments of `xila_process_spawn`. Set `size` to `sizeof(XilaSpawn)`. All
 * pointers are borrowed for the call.
 */
typedef struct XilaSpawn {
  uint32_t size;
  /**
   * Reserved, must be zero.
   */
  uint32_t flags;
  /**
   * Arguments of the child, with no implicit program name. A null array has count zero.
   */
  const struct XilaString *arguments;
  size_t argument_count;
  /**
   * The child's whole environment as `NAME=VALUE` strings, a later entry overriding an
   * earlier one. A null array gives the child a copy of the parent's environment
   * and requires count zero. An array with count zero gives it an empty one.
   */
  const struct XilaString *environment;
  size_t environment_count;
  /**
   * Streams the child gets as duplicates of these. All three are required.
   */
  const struct XilaFile *standard_input;
  const struct XilaFile *standard_output;
  const struct XilaFile *standard_error;
  /**
   * Optional home directory capability the child gets a copy of.
   */
  const struct XilaDirectory *home;
} XilaSpawn;

/**
 * A native worker entry. Null is rejected.
 */
typedef void *(*XilaThreadEntry)(void*);

/**
 * SDK-reserved coroutine-local libc state. Other applications should use TLS keys.
 */
typedef struct XilaThreadLibc {
  int32_t error;
  char *strtok_next;
  uint64_t random_state;
} XilaThreadLibc;

/**
 * Process-local mutex. Zero initialize for a plain mutex, or use mutex_init.
 * Keep its address fixed from first use through destruction. The fields are private
 * to the host; copying a used mutex or sharing it across processes is invalid.
 */
typedef struct XilaMutex {
  size_t owner;
  uint32_t depth;
  bool recursive;
  void *waiters;
} XilaMutex;

/**
 * Process-local condition variable. Zero initialize or use condition_init.
 * Keep its address fixed from first use through destruction. The fields are private
 * to the host; copying a used condition or sharing it across processes is invalid.
 */
typedef struct XilaCondition {
  void *waiters;
} XilaCondition;

/**
 * One poll entry. Only IN and OUT are requested. ERR, HUP and NVAL are
 * reported regardless of the request. IGNORE entries always return zero.
 * A null file/directory/socket or unknown kind reports NVAL.
 */
typedef struct XilaPollEntry {
  /**
   * Opaque object of the type named by kind.
   */
  const void *handle;
  uint32_t kind;
  uint16_t events;
  uint16_t revents;
} XilaPollEntry;

/**
 * Native application entry points, in slot order. The header precedes slot zero.
 * Networking slots return Unsupported when the runtime omits networking, and so does
 * the graphics slot when it omits graphics.
 */
typedef struct XilaApi {
  /**
   * Packed major and minor Xila version: `(major << 16) | minor`.
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
  /**
   * Read a clock's nominal resolution: XILA_CLOCK_MONOTONIC or XILA_CLOCK_REALTIME.
   * Returns Unavailable when the host installed no clock.
   */
  XilaStatus (*time_resolution)(uint32_t clock, uint64_t *nanoseconds);
  /**
   * Suspend for at least the given nanoseconds on the executor's clock. A zero wait
   * yields once inside a coroutine and returns at once outside one.
   */
  XilaStatus (*time_sleep)(uint64_t nanoseconds);
  /**
   * Fill a buffer from the host-granted entropy source. Zero size succeeds. The
   * buffer contents are unspecified after a failure.
   */
  XilaStatus (*random_fill)(void *buffer, size_t size);
  /**
   * Read the process's identifier.
   */
  XilaStatus (*process_id)(const struct XilaProcess *process, uint32_t *id);
  /**
   * Read the identifier of the process that spawned this one, or zero (the root
   * task) once that process has exited.
   */
  XilaStatus (*process_parent_id)(const struct XilaProcess *process, uint32_t *id);
  /**
   * End the process with an exit status. Does not return on success; outside a
   * native process coroutine it returns Unsupported.
   */
  XilaStatus (*process_exit)(const struct XilaProcess *process, int32_t status);
  /**
   * Start an executable below a directory capability as a child process.
   */
  XilaStatus (*process_spawn)(const struct XilaProcess *process,
                              const struct XilaDirectory *directory,
                              const char *path,
                              size_t path_length,
                              const struct XilaSpawn *spawn,
                              struct XilaChild **child);
  /**
   * Collect a child's exit status. Only the spawning process may wait; flags:
   * XILA_WAIT_NO_HANG.
   */
  XilaStatus (*process_wait)(const struct XilaProcess *process,
                             struct XilaChild *child,
                             uint32_t flags,
                             int32_t *status);
  /**
   * Release a child handle. Null succeeds.
   */
  XilaStatus (*process_child_close)(struct XilaChild *child);
  /**
   * Read an environment variable; BufferTooSmall stores the required length.
   */
  XilaStatus (*env_get)(const struct XilaProcess *process,
                        const char *name,
                        size_t name_length,
                        char *buffer,
                        size_t capacity,
                        size_t *value_length);
  /**
   * Set an environment variable of this process.
   */
  XilaStatus (*env_set)(const struct XilaProcess *process,
                        const char *name,
                        size_t name_length,
                        const char *value,
                        size_t value_length);
  /**
   * Remove an environment variable of this process. A missing one succeeds.
   */
  XilaStatus (*env_remove)(const struct XilaProcess *process, const char *name, size_t name_length);
  /**
   * Read a counted NAME=VALUE entry by index; NotFound marks the end.
   */
  XilaStatus (*env_entry)(const struct XilaProcess *process,
                          size_t index,
                          char *buffer,
                          size_t capacity,
                          size_t *entry_length);
  /**
   * Spawn a worker of the current process; stack_size zero chooses the default.
   */
  XilaStatus (*thread_spawn)(const struct XilaProcess *process,
                             size_t stack_size,
                             XilaThreadEntry entry,
                             void *argument,
                             size_t *thread);
  /**
   * Collect the worker result. Only the thread that created it may join.
   */
  XilaStatus (*thread_join)(size_t thread, void **result);
  XilaStatus (*thread_detach)(size_t thread);
  XilaStatus (*thread_yield)(void);
  XilaStatus (*thread_current)(size_t *thread);
  XilaStatus (*thread_key_create)(size_t *key);
  XilaStatus (*thread_key_delete)(size_t key);
  XilaStatus (*thread_tls_get)(size_t key, void **value);
  XilaStatus (*thread_tls_set)(size_t key, void *value);
  /**
   * Address of the SDK-reserved libc state, valid until the current thread ends.
   */
  XilaStatus (*thread_libc)(struct XilaThreadLibc **location);
  XilaStatus (*mutex_init)(struct XilaMutex *mutex, uint32_t flags);
  XilaStatus (*mutex_destroy)(struct XilaMutex *mutex);
  /**
   * A contended XILA_MUTEX_TRY returns WouldBlock.
   */
  XilaStatus (*mutex_lock)(struct XilaMutex *mutex, uint32_t flags);
  XilaStatus (*mutex_unlock)(struct XilaMutex *mutex);
  XilaStatus (*condition_init)(struct XilaCondition *condition);
  XilaStatus (*condition_destroy)(struct XilaCondition *condition);
  /**
   * Unlock and sleep atomically, then reacquire before returning. A null deadline
   * means infinite; otherwise use absolute nanoseconds on the selected ABI clock.
   */
  XilaStatus (*condition_wait)(struct XilaCondition *condition,
                               struct XilaMutex *mutex,
                               uint32_t clock,
                               const int64_t *deadline);
  XilaStatus (*condition_notify)(struct XilaCondition *condition, bool all);
  /**
   * Report whether any worker is live, including the caller if it is a worker.
   */
  XilaStatus (*thread_has_workers)(bool *live);
  /**
   * Begin terminal cleanup. The process driver will only resume the caller.
   */
  XilaStatus (*thread_stop_others)(const struct XilaProcess *process);
  /**
   * End a worker with a join result. This returns only on failure.
   */
  XilaStatus (*thread_exit)(void *result);
  /**
   * Wait for readiness across file, pipe, directory and socket handles.
   * timeout_ms is relative; negative means infinite, zero probes once.
   */
  XilaStatus (*poll_wait)(struct XilaPollEntry *entries,
                          size_t count,
                          int32_t timeout_ms,
                          size_t *ready_count);
  /**
   * Transfer from an explicit offset without changing the shared cursor.
   */
  XilaStatus (*file_read_at)(struct XilaFile *file,
                             void *buffer,
                             size_t size,
                             uint64_t offset,
                             size_t *count);
  /**
   * Transfer to an explicit offset without changing the shared cursor.
   */
  XilaStatus (*file_write_at)(struct XilaFile *file,
                              const void *buffer,
                              size_t size,
                              uint64_t offset,
                              size_t *count);
  /**
   * Return an owned file that shares the original cursor and status flags.
   */
  XilaStatus (*file_duplicate)(const struct XilaFile *file, struct XilaFile **output);
  /**
   * Query queued pipe bytes or get/set file flags using a uint32_t buffer.
   */
  XilaStatus (*file_control)(struct XilaFile *file,
                             uint32_t command,
                             const void *input,
                             size_t input_size,
                             void *output,
                             size_t output_size);
  /**
   * Report whether an open file is an interactive terminal.
   */
  XilaStatus (*file_is_terminal)(struct XilaFile *file, bool *terminal);
  /**
   * Create the read and write handles of an unnamed pipe.
   * The only supported flag is XILA_OPEN_NON_BLOCKING.
   */
  XilaStatus (*pipe_create)(const struct XilaProcess *process,
                            uint32_t flags,
                            struct XilaFile **reader,
                            struct XilaFile **writer);
  /**
   * Run one graphics call by its ID from `xila/graphics.h`. `arguments` holds
   * `argument_count` words (at most seven; null at zero) and `result` receives the
   * call's word or 32-bit result. The SDK's generated functions pack the words.
   */
  XilaStatus (*graphics_call)(const struct XilaProcess *process,
                              uint32_t function,
                              const size_t *arguments,
                              size_t argument_count,
                              void *result);
  /**
   * Read a child's process identifier, the value its `process_id` reports.
   */
  XilaStatus (*process_child_id)(const struct XilaChild *child, uint32_t *id);
} XilaApi;

/**
 * Host-owned startup data, valid only for the duration of the entry call.
 * The API is always present. Capability pointers may be null when not granted;
 * the process capability is null for an application that is not a process, such
 * as a built-in one linked into the firmware, whose process calls fail with
 * InvalidArgument. A null argument array has argument_count zero.
 * Fields are only ever appended: check `size` before reading a field that a
 * later Xila version added, such as `network`.
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
  const struct XilaProcess *process;
  /**
   * Executable's launch name. Empty for a linked entry without a process.
   */
  struct XilaString program_name;
  /**
   * Network authority granted to the process: connect, listen and resolve. Null when
   * the host has no network stack, the firmware was built without networking, or the
   * application is not a process. Appended after `program_name`, so images built
   * before it existed still start.
   */
  const struct XilaNetwork *network;
} XilaStartup;

#endif  /* XILA_APPLICATION_H */
