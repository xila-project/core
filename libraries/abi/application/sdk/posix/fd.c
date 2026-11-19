/* Per-process integer descriptor table.
 *
 * The table is an array of {pointer, kind, flags} on the process heap: 8 bytes per
 * slot on 32-bit targets, eight slots at first, doubling on demand up to
 * XILA_OPEN_MAX. A process that never opens a file past the standard streams
 * pays 64 bytes. */
#include <string.h>

#include "internal.h"

#define XILA_FD_INITIAL 8
#define XILA_OPEN_MAX 256

struct xila_process xila_process;

static int grow(size_t wanted) {
    size_t count = xila_process.fd_count;
    if (wanted > XILA_OPEN_MAX)
        return -1;
    size_t next = count == 0 ? XILA_FD_INITIAL : count * 2;
    while (next < wanted)
        next *= 2;
    if (next > XILA_OPEN_MAX)
        next = XILA_OPEN_MAX;
    void *allocation = NULL;
    if (xila_heap_reallocate(xila_process.startup->heap, xila_process.fds,
                             next * sizeof(struct xila_fd), &allocation) != XILA_STATUS_OK)
        return -1;
    xila_process.fds = allocation;
    memset(xila_process.fds + count, 0, (next - count) * sizeof(struct xila_fd));
    xila_process.fd_count = (uint16_t)next;
    return 0;
}

int xila_fd_initialize(const XilaStartup *startup) {
    xila_process.startup = startup;
    if (startup->heap == NULL)
        return -1;
    if (grow(XILA_FD_INITIAL) != 0)
        return -1;
    XilaFile *standard[3] = {startup->standard_input, startup->standard_output,
                             startup->standard_error};
    for (int fd = 0; fd < 3; ++fd) {
        if (standard[fd] != NULL) {
            xila_process.fds[fd].object = standard[fd];
            xila_process.fds[fd].kind = XILA_FD_FILE;
            xila_process.fds[fd].flags = XILA_FD_BORROWED;
        }
    }
    return 0;
}

int xila_fd_allocate(void *object, uint8_t kind, uint8_t flags) {
    for (;;) {
        for (size_t fd = 0; fd < xila_process.fd_count; ++fd) {
            if (xila_process.fds[fd].object == NULL) {
                xila_process.fds[fd].object = object;
                xila_process.fds[fd].kind = kind;
                xila_process.fds[fd].flags = flags;
                return (int)fd;
            }
        }
        if (grow((size_t)xila_process.fd_count + 1) != 0) {
            errno = xila_process.fd_count >= XILA_OPEN_MAX ? EMFILE : ENOMEM;
            return -1;
        }
    }
}

struct xila_fd *xila_fd_get(int fd) {
    if (fd < 0 || (size_t)fd >= xila_process.fd_count || xila_process.fds[fd].object == NULL) {
        errno = EBADF;
        return NULL;
    }
    return &xila_process.fds[fd];
}

/* Release everything the application left open. Borrowed streams stay with the host. */
void xila_fd_close_all(void) {
    for (size_t fd = 0; fd < xila_process.fd_count; ++fd) {
        struct xila_fd *entry = &xila_process.fds[fd];
        if (entry->object != NULL && !(entry->flags & XILA_FD_BORROWED)) {
            if (entry->kind == XILA_FD_FILE)
                (void)xila_file_close(entry->object);
            else
                (void)xila_directory_close(entry->object);
        }
        entry->object = NULL;
    }
    xila_heap_release(xila_process.fds);
    xila_process.fds = NULL;
    xila_process.fd_count = 0;
}
