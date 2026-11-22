/* Internal declarations shared by the libxila_posix translation units. */
#ifndef XILA_POSIX_INTERNAL_H
#define XILA_POSIX_INTERNAL_H

#include <errno.h>
#include <stddef.h>
#include <stdint.h>

#include "stubs.h"

/* Longest absolute path (including the terminating NUL) the layer resolves. Paths are
   normalized into a buffer of this size on the caller's stack. */
#define XILA_PATH_MAX 256

/* Descriptor kinds. A free slot has object == NULL. */
enum { XILA_FD_FILE = 1, XILA_FD_DIRECTORY = 2 };

/* The three startup streams are borrowed from the host: close() detaches without
   releasing them. */
#define XILA_FD_BORROWED 1
#define XILA_FD_EXIT_FLUSH 2
#define XILA_FD_CLOEXEC 4
#define XILA_OPEN_MAX 256

struct xila_fd {
    void *object; /* XilaFile * or XilaDirectory * */
    uint8_t kind;
    uint8_t flags;
};

/* Process state. Small and zero-initialized: the descriptor table lives on the process heap. */
struct xila_process {
    const XilaStartup *startup;
    /* startup->process, copied to static storage at startup so the stack smash handler
       does not read it back from a stack that may have been overwritten. */
    const XilaProcess *process;
    struct xila_fd *fds;
    uint16_t fd_count;
    char *cwd; /* NULL means "/" */
};

extern struct xila_process xila_process;

/* The identifier of a process's main thread. Workers get larger ones. */
#define XILA_MAIN_THREAD 1

/* Convert a native status to errno. Returns -1 so callers can `return xila_fail(status)`. */
int xila_fail(XilaStatus status);
int xila_errno_from_status(XilaStatus status);
/* Set errno and return -1. */
int xila_fail_errno(int error);

/* Convert an absolute deadline to nanoseconds. Returns 0, EINVAL for a null deadline or
   an out-of-range tv_nsec, or EOVERFLOW. */
struct timespec;
int xila_deadline_nanoseconds(const struct timespec *deadline, int64_t *nanoseconds);

/* Process heap used by malloc and by the layer itself. */
void *xila_heap_alloc(size_t size);
void xila_heap_release(void *pointer);

/* Descriptor table. */
int xila_fd_allocate(void *object, uint8_t kind, uint8_t flags);
struct xila_fd *xila_fd_get(int fd);
void xila_fd_keep_for_exit_flush(int fd);
void xila_fd_close_before_exit_flush(void);
void xila_fd_close_all(void);
int xila_fd_initialize(const XilaStartup *startup);

/* Path resolution: normalize `path` against the current directory (AT_FDCWD) or a
   directory descriptor and produce the capability plus a relative path inside `buffer`. */
struct xila_path {
    const XilaDirectory *directory;
    const char *relative;
    size_t length;
};
int xila_path_resolve(int dirfd, const char *path, char *buffer, struct xila_path *output);
const char *xila_cwd(void);
int xila_cwd_set(const char *absolute);
/* Normalize `path` onto the absolute `base`. Returns 0 or an errno value. */
int xila_path_normalize(const char *base, const char *path, char *output, int allow_escape);

/* Commit a validated POSIX environment mutation with the libc lock held.
   A null value removes the name. */
int xila_posix_env_commit(const char *name, size_t name_length, const char *value);

/* Startup and shutdown. */
int xila_posix_initialize(const XilaStartup *startup, int *argc, char ***argv);
void xila_posix_shutdown(void);
void xila_stdio_exit_with_stopped_threads(void);

static inline XilaThreadLibc *xila_libc_state(void) {
    XilaThreadLibc *state = NULL;
    (void)xila_thread_libc(&state);
    return state;
}

#endif
