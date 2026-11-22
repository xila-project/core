#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <unistd.h>

#include "internal.h"

_Static_assert(PIPE_BUF == XILA_PIPE_CAPACITY, "SDK and ABI atomic pipe write sizes must agree");

int pipe2(int fds[2], int flags) {
    if (fds == NULL)
        return xila_fail_errno(EFAULT);
    if (flags & ~(O_NONBLOCK | O_CLOEXEC))
        return xila_fail_errno(EINVAL);

    XilaFile *reader = NULL, *writer = NULL;
    uint32_t native = (flags & O_NONBLOCK) ? XILA_OPEN_NON_BLOCKING : 0;
    XilaStatus status = xila_pipe_create(xila_process.process, native, &reader, &writer);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);

    uint8_t descriptor_flags = (flags & O_CLOEXEC) ? XILA_FD_CLOEXEC : 0;
    int input = xila_fd_allocate(reader, XILA_FD_FILE, descriptor_flags);
    int output = input < 0 ? -1 : xila_fd_allocate(writer, XILA_FD_FILE, descriptor_flags);
    if (output < 0) {
        int error = errno;
        if (input >= 0)
            (void)close(input);
        else
            (void)xila_file_close(reader);
        (void)xila_file_close(writer);
        return xila_fail_errno(error);
    }
    fds[0] = input;
    fds[1] = output;
    return 0;
}

int pipe(int fds[2]) { return pipe2(fds, 0); }

static uint16_t native_events(short events) {
    uint16_t native = 0;
    if (events & (POLLIN | POLLRDNORM))
        native |= XILA_POLL_IN;
    if (events & (POLLOUT | POLLWRNORM))
        native |= XILA_POLL_OUT;
    return native;
}

static short posix_events(uint16_t native, short requested) {
    short events = 0;
    if (native & XILA_POLL_IN)
        events |= requested & (POLLIN | POLLRDNORM);
    if (native & XILA_POLL_OUT)
        events |= requested & (POLLOUT | POLLWRNORM);
    if (native & XILA_POLL_ERR)
        events |= POLLERR;
    if (native & XILA_POLL_HUP)
        events |= POLLHUP;
    if (native & XILA_POLL_NVAL)
        events |= POLLNVAL;
    return events;
}

int poll(struct pollfd *fds, nfds_t count, int timeout) {
    if (count > XILA_OPEN_MAX)
        return xila_fail_errno(EINVAL);
    if (count != 0 && fds == NULL)
        return xila_fail_errno(EFAULT);

    XilaPollEntry *entries = NULL;
    if (count) {
        entries = xila_heap_alloc(count * sizeof(*entries));
        if (entries == NULL)
            return -1;
    }

    int saved_errno = errno;
    for (nfds_t i = 0; i < count; ++i) {
        if (fds[i].fd < 0) {
            entries[i] = (XilaPollEntry){.kind = XILA_POLL_IGNORE};
            continue;
        }
        struct xila_fd *entry = xila_fd_get(fds[i].fd);
        entries[i] = (XilaPollEntry){
            .handle = entry == NULL ? NULL : entry->object,
            .kind = entry != NULL && entry->kind == XILA_FD_DIRECTORY
                        ? XILA_POLL_DIRECTORY : XILA_POLL_FILE,
            .events = native_events(fds[i].events),
        };
        if (entry != NULL && entry->kind == XILA_FD_SOCKET)
            xila_socket_poll_entry(entry->object, &entries[i]);
    }
    errno = saved_errno;

    size_t ready = 0;
    XilaStatus status = xila_poll_wait(entries, count, timeout, &ready);
    if (status == XILA_STATUS_OK) {
        for (nfds_t i = 0; i < count; ++i)
            fds[i].revents = posix_events(entries[i].revents, fds[i].events);
    }
    xila_heap_release(entries);
    return status == XILA_STATUS_OK ? (int)ready : xila_fail(status);
}
