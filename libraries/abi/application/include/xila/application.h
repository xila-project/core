#ifndef XILA_APPLICATION_H
#define XILA_APPLICATION_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define XILA_ABI_VERSION_MAJOR 0

#define XILA_ABI_VERSION_MINOR 3

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

/**
 * Stable error values; outputs change only on success except the documented
 * BufferTooSmall required listing length.
 */
enum XilaStatus
#if __STDC_VERSION__ >= 202311L
  : uint32_t
#endif // __STDC_VERSION__ >= 202311L
 {
  XILA_STATUS_OK = 0,
  XILA_STATUS_INVALID_ARGUMENT = 1,
  XILA_STATUS_NOT_FOUND = 2,
  XILA_STATUS_PERMISSION_DENIED = 3,
  XILA_STATUS_ALREADY_EXISTS = 4,
  XILA_STATUS_NOT_A_DIRECTORY = 5,
  XILA_STATUS_IS_A_DIRECTORY = 6,
  XILA_STATUS_NO_SPACE = 7,
  XILA_STATUS_OUT_OF_MEMORY = 8,
  XILA_STATUS_WOULD_BLOCK = 9,
  XILA_STATUS_UNSUPPORTED = 10,
  XILA_STATUS_OVERFLOW = 11,
  XILA_STATUS_UNAVAILABLE = 12,
  XILA_STATUS_INPUT_OUTPUT = 13,
  XILA_STATUS_END = 14,
  XILA_STATUS_BUFFER_TOO_SMALL = 15,
  XILA_STATUS_DIRECTORY_NOT_EMPTY = 16,
};
#if __STDC_VERSION__ >= 202311L
typedef enum XilaStatus XilaStatus;
#else
typedef uint32_t XilaStatus;
#endif // __STDC_VERSION__ >= 202311L

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
 * UTF-8 bytes, not NUL-terminated. A null data pointer is valid only at length zero.
 */
typedef struct XilaString {
  const char *data;
  size_t length;
} XilaString;

/**
 * Host-owned startup data, valid only for the duration of the entry call.
 * Any pointer may be null when not granted (with zero argument_count for arguments).
 * Applications must not close, retain or transfer these objects; closing them is
 * undefined behavior. Objects opened by the application must still be closed.
 */
typedef struct XilaStartup {
  uint32_t size;
  const struct XilaString *arguments;
  size_t argument_count;
  struct XilaFile *standard_input;
  struct XilaFile *standard_output;
  struct XilaFile *standard_error;
  struct XilaDirectory *home;
} XilaStartup;

/**
 * Return the ABI version as (major << 16) | minor.
 * # Safety
 * No preconditions.
 */
uint32_t xila_abi_version(void);

/**
 * Open a relative directory; requested rights must be a subset of the parent's.
 * # Safety
 * Non-null objects must be live. Path must be readable for its length and output
 * writable and aligned. Outputs must not overlap inputs. Calls must be serialized.
 */
XilaStatus xila_directory_open(const struct XilaDirectory *parent,
                               const char *path,
                               size_t path_length,
                               uint32_t rights,
                               struct XilaDirectory **directory);

/**
 * Release a directory capability. Null is a no-op.
 * # Safety
 * Non-null pointer must be owned, live, and released exactly once; no concurrent calls.
 */
void xila_directory_close(struct XilaDirectory *directory);

/**
 * Create a relative directory with the WRITE right.
 * # Safety
 * Directory must be live; path readable for length. Calls must be serialized.
 */
XilaStatus xila_directory_create(const struct XilaDirectory *directory,
                                 const char *path,
                                 size_t length);

/**
 * Remove a relative entry; XILA_REMOVE_DIRECTORY permits directory removal.
 * # Safety
 * Directory must be live; path readable for length. Calls must be serialized.
 */
XilaStatus xila_directory_remove(const struct XilaDirectory *directory,
                                 const char *path,
                                 size_t length,
                                 uint32_t flags);

/**
 * Rename an entry using two WRITE capabilities with the same VFS and task.
 * Existing destinations follow the underlying filesystem's replacement semantics.
 * # Safety
 * Objects must be live and paths readable for their lengths. Calls must be serialized.
 */
XilaStatus xila_directory_rename(const struct XilaDirectory *from,
                                 const char *from_path,
                                 size_t from_length,
                                 const struct XilaDirectory *to,
                                 const char *to_path,
                                 size_t to_length);

/**
 * Open a file relative to a directory with validated XILA_OPEN flags.
 * # Safety
 * Objects must be live. Path must be readable for its length; output writable,
 * aligned and disjoint from inputs. Calls on an object must be serialized.
 */
XilaStatus xila_file_open(const struct XilaDirectory *directory,
                          const char *path,
                          size_t path_length,
                          uint32_t flags,
                          struct XilaFile **file);

/**
 * Read directly into the caller's buffer; zero size permits a null buffer.
 * # Safety
 * File must be live and exclusively accessed. Buffer must be writable for size
 * bytes and disjoint from file and count. Count must be aligned and writable.
 */
XilaStatus xila_file_read(struct XilaFile *file, void *buffer, size_t size, size_t *count);

/**
 * Write directly from the caller's buffer; zero size permits a null buffer.
 * # Safety
 * File must be live and exclusively accessed. Buffer must be readable for size
 * bytes and disjoint from file and count. Count must be aligned and writable.
 */
XilaStatus xila_file_write(struct XilaFile *file, const void *buffer, size_t size, size_t *count);

/**
 * Seek using XILA_SEEK_START, CURRENT or END. Position output is optional.
 * # Safety
 * File must be live and exclusively accessed; non-null position must be aligned,
 * writable and disjoint from the object.
 */
XilaStatus xila_file_seek(struct XilaFile *file,
                          int64_t offset,
                          uint32_t whence,
                          uint64_t *position);

/**
 * Flush pending writes.
 * # Safety
 * File must be live and exclusively accessed.
 */
XilaStatus xila_file_flush(struct XilaFile *file);

/**
 * Close and free the object even if closing fails. Null succeeds.
 * # Safety
 * Non-null file must be owned and released exactly once, with no concurrent calls.
 */
XilaStatus xila_file_close(struct XilaFile *file);

/**
 * Resize a writable file without changing its cursor.
 * # Safety
 * File must be live and exclusively accessed.
 */
XilaStatus xila_file_set_size(struct XilaFile *file, uint64_t size);

/**
 * Open a relative directory for listing with READ authority; empty means itself.
 * # Safety
 * Directory must be live; path readable for length. Output must be aligned,
 * writable and disjoint from inputs. Calls must be serialized.
 */
XilaStatus xila_directory_iterator_open(const struct XilaDirectory *directory,
                                        const char *path,
                                        size_t length,
                                        struct XilaDirectoryIterator **output);

/**
 * Copy a name without a trailing NUL, skipping dot entries. End leaves outputs
 * unchanged. BufferTooSmall writes only required name_length and retains the entry.
 * # Safety
 * Iterator must be live and exclusively accessed. Buffer must be writable for
 * capacity bytes (null allowed only at zero capacity). Outputs must be aligned,
 * writable and disjoint from each other, the buffer and iterator.
 */
XilaStatus xila_directory_iterator_next(struct XilaDirectoryIterator *iterator,
                                        char *name_buffer,
                                        size_t capacity,
                                        size_t *name_length,
                                        uint32_t *kind);

/**
 * Rewind, discarding a pending entry only on success.
 * # Safety
 * Iterator must be live and exclusively accessed.
 */
XilaStatus xila_directory_iterator_rewind(struct XilaDirectoryIterator *iterator);

/**
 * Close and free the iterator even on close error; null succeeds.
 * # Safety
 * Non-null iterator must be owned, live and released once; no concurrent calls.
 */
XilaStatus xila_directory_iterator_close(struct XilaDirectoryIterator *iterator);

/**
 * Read metadata of an opened readable file.
 * # Safety
 * File must be live and exclusively accessed. Output must be aligned, writable
 * for its declared size, initialized with size, and disjoint from the file.
 */
XilaStatus xila_file_get_statistics(struct XilaFile *file, struct XilaStatistics *output);

/**
 * Read relative path metadata; an empty path means the capability itself.
 * # Safety
 * Directory must be live; path readable for length. Output must be aligned,
 * writable for its declared size, initialized with size and disjoint from inputs.
 * Calls must be serialized.
 */
XilaStatus xila_directory_get_statistics(const struct XilaDirectory *directory,
                                         const char *path,
                                         size_t length,
                                         uint32_t flags,
                                         struct XilaStatistics *output);

/**
 * Read monotonic nanoseconds from an arbitrary epoch; may return Overflow.
 * # Safety
 * Output must be aligned and writable.
 */
XilaStatus xila_time_monotonic(uint64_t *nanoseconds);

/**
 * Read signed Unix UTC nanoseconds; unsynchronized clocks return Unavailable.
 * # Safety
 * Output must be aligned and writable.
 */
XilaStatus xila_time_realtime(int64_t *nanoseconds);

#endif  /* XILA_APPLICATION_H */
