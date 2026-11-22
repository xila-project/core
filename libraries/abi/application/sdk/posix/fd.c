/* Per-process integer descriptor table.
 *
 * The table is an array of {pointer, kind, flags} on the process heap: 8 bytes per
 * slot on 32-bit targets, eight slots at first, doubling on demand up to
 * XILA_OPEN_MAX. A process that never opens a file past the standard streams
 * pays 64 bytes. */
#include <fcntl.h>
#include <stdarg.h>
#include <string.h>
#include <unistd.h>

#include "internal.h"

#define XILA_FD_INITIAL 8

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
    xila_process.process = startup->process;
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

static int allocate_from(struct xila_fd entry, size_t minimum) {
    for (;;) {
        for (size_t fd = minimum; fd < xila_process.fd_count; ++fd) {
            if (xila_process.fds[fd].object == NULL) {
                xila_process.fds[fd] = entry;
                return (int)fd;
            }
        }
        size_t wanted = (size_t)xila_process.fd_count + 1;
        if (wanted <= minimum)
            wanted = minimum + 1;
        if (grow(wanted) != 0) {
            errno = xila_process.fd_count >= XILA_OPEN_MAX ? EMFILE : ENOMEM;
            return -1;
        }
    }
}

int xila_fd_allocate(void *object, uint8_t kind, uint8_t flags) {
    return allocate_from((struct xila_fd){object, kind, flags}, 0);
}

struct xila_fd *xila_fd_get(int fd) {
    if (fd < 0 || (size_t)fd >= xila_process.fd_count || xila_process.fds[fd].object == NULL) {
        errno = EBADF;
        return NULL;
    }
    return &xila_process.fds[fd];
}

static XilaStatus dispose(struct xila_fd entry) {
    if (entry.object == NULL || (entry.flags & XILA_FD_BORROWED))
        return XILA_STATUS_OK;
    if (entry.kind == XILA_FD_SOCKET)
        return xila_socket_release(entry.object);
    return entry.kind == XILA_FD_FILE ? xila_file_close(entry.object)
                                     : xila_directory_close(entry.object);
}

int close(int fd) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return -1;
    struct xila_fd previous = *entry;
    entry->object = NULL;
    XilaStatus status = dispose(previous);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

static XilaStatus duplicate(struct xila_fd entry, uint8_t flags, struct xila_fd *copy) {
    *copy = (struct xila_fd){.kind = entry.kind, .flags = flags};
    if (entry.kind == XILA_FD_SOCKET) {
        xila_socket_retain(entry.object);
        copy->object = entry.object;
        return XILA_STATUS_OK;
    }
    if (entry.kind == XILA_FD_FILE) {
        XilaFile *file = NULL;
        XilaStatus status = xila_file_duplicate(entry.object, &file);
        copy->object = file;
        return status;
    }
    XilaDirectory *directory = NULL;
    XilaStatus status = xila_directory_open(entry.object, "", 0, XILA_RIGHT_READ, &directory);
    copy->object = directory;
    return status;
}

static int duplicate_from(int fd, int minimum, uint8_t flags) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return -1;
    if (minimum < 0 || minimum >= XILA_OPEN_MAX)
        return xila_fail_errno(EINVAL);
    struct xila_fd copy;
    XilaStatus status = duplicate(*entry, flags, &copy);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    int next = allocate_from(copy, (size_t)minimum);
    if (next < 0) {
        int error = errno;
        (void)dispose(copy);
        errno = error;
    }
    return next;
}

int dup(int fd) { return duplicate_from(fd, 0, 0); }

int dup2(int fd, int target) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return -1;
    if (target < 0 || target >= XILA_OPEN_MAX)
        return xila_fail_errno(EBADF);
    if (fd == target)
        return target;

    struct xila_fd source = *entry;
    if ((size_t)target >= xila_process.fd_count && grow((size_t)target + 1) != 0)
        return xila_fail_errno(ENOMEM);

    struct xila_fd copy;
    XilaStatus status = duplicate(source, 0, &copy);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    struct xila_fd previous = xila_process.fds[target];
    xila_process.fds[target] = copy;
    (void)dispose(previous);
    return target;
}

static int get_file_flags(const struct xila_fd *entry) {
    if (entry->kind == XILA_FD_DIRECTORY)
        return O_RDONLY;
    if (entry->kind == XILA_FD_SOCKET)
        return xila_socket_get_flags(entry->object);
    uint32_t flags = 0;
    XilaStatus status = xila_file_control(entry->object, XILA_CONTROL_GET_FILE_FLAGS,
                                          NULL, 0, &flags, sizeof(flags));
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    int access = O_RDONLY;
    if (flags & XILA_OPEN_WRITE)
        access = (flags & XILA_OPEN_READ) ? O_RDWR : O_WRONLY;
    return access | ((flags & XILA_OPEN_NON_BLOCKING) ? O_NONBLOCK : 0) |
           ((flags & XILA_OPEN_APPEND) ? O_APPEND : 0);
}

static int set_file_flags(const struct xila_fd *entry, int flags) {
    if (entry->kind == XILA_FD_DIRECTORY)
        return xila_fail_errno(EINVAL);
    if (entry->kind == XILA_FD_SOCKET) {
        xila_socket_set_flags(entry->object, flags);
        return 0;
    }
    uint32_t native = ((flags & O_NONBLOCK) ? XILA_OPEN_NON_BLOCKING : 0) |
                      ((flags & O_APPEND) ? XILA_OPEN_APPEND : 0);
    XilaStatus status = xila_file_control(entry->object, XILA_CONTROL_SET_FILE_FLAGS,
                                          &native, sizeof(native), NULL, 0);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

int fcntl(int fd, int command, ...) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return -1;
    if (command == F_GETFD)
        return (entry->flags & XILA_FD_CLOEXEC) ? FD_CLOEXEC : 0;
    if (command == F_GETFL)
        return get_file_flags(entry);
    if (command != F_DUPFD && command != F_DUPFD_CLOEXEC && command != F_SETFD &&
        command != F_SETFL)
        return xila_fail_errno(EINVAL);
    va_list arguments;
    va_start(arguments, command);
    int value = va_arg(arguments, int);
    va_end(arguments);
    if (command == F_SETFL)
        return set_file_flags(entry, value);
    if (command == F_SETFD) {
        entry->flags = (entry->flags & ~XILA_FD_CLOEXEC) |
                       ((value & FD_CLOEXEC) ? XILA_FD_CLOEXEC : 0);
        return 0;
    }
    return duplicate_from(fd, value, command == F_DUPFD_CLOEXEC ? XILA_FD_CLOEXEC : 0);
}

/* Forget borrowed streams without releasing them. */
static void close_descriptors(uint8_t skip) {
    for (size_t fd = 0; fd < xila_process.fd_count; ++fd) {
        struct xila_fd *entry = &xila_process.fds[fd];
        if (entry->object == NULL || (entry->flags & skip))
            continue;
        struct xila_fd previous = *entry;
        entry->object = NULL;
        (void)dispose(previous);
    }
}

void xila_fd_keep_for_exit_flush(int fd) {
    if (fd >= 0 && (size_t)fd < xila_process.fd_count)
        xila_process.fds[fd].flags |= XILA_FD_EXIT_FLUSH;
}

void xila_fd_close_before_exit_flush(void) { close_descriptors(XILA_FD_EXIT_FLUSH); }

void xila_fd_close_all(void) {
    close_descriptors(0);
    xila_heap_release(xila_process.fds);
    xila_process.fds = NULL;
    xila_process.fd_count = 0;
}
