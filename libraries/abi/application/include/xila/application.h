#ifndef XILA_APPLICATION_H
#define XILA_APPLICATION_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define XILA_ABI_VERSION_MAJOR 0

#define XILA_ABI_VERSION_MINOR 2

#define XILA_RIGHT_READ 1

#define XILA_RIGHT_WRITE 2

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

#define XILA_MUTEX_RECURSIVE 1

/**
 * Stable error values; output arguments are written only on success.
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
  XILA_STATUS_TIMED_OUT = 20,
  XILA_STATUS_DEADLOCK = 21,
  XILA_STATUS_BUSY = 22,
};
#if __STDC_VERSION__ >= 202311L
typedef enum XilaStatus XilaStatus;
#else
typedef uint32_t XilaStatus;
#endif // __STDC_VERSION__ >= 202311L

/**
 * Opaque condition variable. Always wait in a predicate loop.
 */
typedef struct XilaConditionVariable XilaConditionVariable;

/**
 * Opaque path capability. Native callers must serialize access and preserve ownership.
 */
typedef struct XilaDirectory XilaDirectory;

/**
 * Opaque owned file. No handle lookup or buffer allocation is required for I/O.
 */
typedef struct XilaFile XilaFile;

/**
 * Opaque native mutex. Ownership is per OS thread, not per Xila task.
 */
typedef struct XilaMutex XilaMutex;

/**
 * Owned native thread join token.
 */
typedef struct XilaThread XilaThread;

/**
 * Extensible thread configuration. Zero stack size selects the platform default.
 */
typedef struct XilaThreadOptions {
  uint32_t size;
  size_t stack_size;
  const char *name;
  size_t name_length;
} XilaThreadOptions;

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
 * Allocate a mutex once; unknown flags are invalid.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_mutex_create(uint32_t flags, struct XilaMutex **mutex);

/**
 * Lock, sleeping while contended. Nonrecursive self-lock returns Deadlock.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_mutex_lock(struct XilaMutex *mutex);

/**
 * Probe once; contention returns WouldBlock.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_mutex_try_lock(struct XilaMutex *mutex);

/**
 * Unlock; only the owning native thread may unlock.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_mutex_unlock(struct XilaMutex *mutex);

/**
 * Free an unlocked mutex. Busy leaves ownership with the caller.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_mutex_destroy(struct XilaMutex *mutex);

/**
 * Allocate a condition variable.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_condvar_create(struct XilaConditionVariable **condition);

/**
 * Atomically release and reacquire mutex (held exactly once). Timeout also reacquires it.
 * All waits on this condition variable must use the same mutex.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_condvar_wait(struct XilaConditionVariable *condition, struct XilaMutex *mutex);

/**
 * Atomically release and reacquire mutex (held exactly once). Timeout also reacquires it.
 * All waits on this condition variable must use the same mutex.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_condvar_wait_timeout(struct XilaConditionVariable *condition,
                                     struct XilaMutex *mutex,
                                     uint64_t nanoseconds);

/**
 * Wake one waiter; notifications are not saved.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_condvar_signal(struct XilaConditionVariable *condition);

/**
 * Wake all current waiters.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_condvar_broadcast(struct XilaConditionVariable *condition);

/**
 * Free a condition variable with no waiters.
 * # Safety
 * Objects must be live, aligned and remain live throughout the call. Outputs
 * must be writable and disjoint from inputs. Destroy requires exclusive ownership,
 * no waiters and no concurrent calls. Blocking calls require a dedicated native thread.
 */
XilaStatus xila_condvar_destroy(struct XilaConditionVariable *condition);

/**
 * Start a native OS thread. Unsupported on targets without a stackful backend.
 * # Safety
 * Options must be readable and aligned, including its declared size. Name must
 * be readable for its length. Output must be writable and disjoint from inputs.
 * Entry and argument must remain valid until entry returns, including after detach.
 */
XilaStatus xila_thread_spawn(int32_t (*entry)(void*),
                             void *argument,
                             const struct XilaThreadOptions *options,
                             struct XilaThread **thread);

/**
 * Join and consume a thread. Validation and self-join errors do not consume it.
 * # Safety
 * Thread must be uniquely owned and live. Result must be aligned, writable and
 * disjoint from thread. No concurrent join/detach is permitted.
 */
XilaStatus xila_thread_join(struct XilaThread *thread, int32_t *result);

/**
 * Consume a join token; the thread continues and releases its resources on exit.
 * # Safety
 * Thread must be uniquely owned and live, with no concurrent join/detach.
 */
XilaStatus xila_thread_detach(struct XilaThread *thread);

/**
 * Sleep for at least the supplied duration using the OS scheduler.
 * # Safety
 * Call on a dedicated native thread, not inside a cooperative executor poll.
 */
XilaStatus xila_thread_sleep(uint64_t nanoseconds);

/**
 * Yield the calling OS thread to the scheduler.
 * # Safety
 * No preconditions.
 */
XilaStatus xila_thread_yield(void);

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
