/* Path resolution: POSIX strings to capability + pointer/length.
 *
 * The startup home directory provides the root capability. Absolute paths and
 * AT_FDCWD use the SDK's lexical current directory. Paths relative to an open
 * directory reach the Core resolver with their components intact. */
#include <fcntl.h>
#include <string.h>

#include "internal.h"

static const XilaDirectory *root(void) {
    return xila_process.startup != NULL ? xila_process.startup->home : NULL;
}

const char *xila_cwd(void) { return xila_process.cwd != NULL ? xila_process.cwd : "/"; }

int xila_cwd_set(const char *absolute) {
    size_t length = strlen(absolute) + 1;
    char *copy = xila_heap_alloc(length);
    if (copy == NULL)
        return -1;
    memcpy(copy, absolute, length);
    xila_heap_release(xila_process.cwd);
    xila_process.cwd = copy;
    return 0;
}

/* Append the components of `path` to `output` (an absolute normalized path with room
   for XILA_PATH_MAX bytes). With allow_escape clear, ".." above the start of the
   path fails with the errno of a path-escape status instead of stopping at the root. */
static int append(char *output, size_t *length, const char *path, int allow_escape) {
    const char *cursor = path;
    while (*cursor != '\0') {
        while (*cursor == '/')
            ++cursor;
        const char *start = cursor;
        while (*cursor != '\0' && *cursor != '/')
            ++cursor;
        size_t size = (size_t)(cursor - start);
        if (size == 0 || (size == 1 && start[0] == '.'))
            continue;
        if (size == 2 && start[0] == '.' && start[1] == '.') {
            if (*length <= 1)
                return allow_escape ? 0 : xila_errno_from_status(XILA_STATUS_PATH_ESCAPES_ROOT);
            while (*length > 1 && output[*length - 1] != '/')
                --*length;
            if (*length > 1)
                --*length; /* drop the separator before the removed component */
            output[*length] = '\0';
            continue;
        }
        size_t separator = *length > 1 ? 1 : 0;
        if (*length + separator + size + 1 > XILA_PATH_MAX)
            return ENAMETOOLONG;
        if (separator)
            output[(*length)++] = '/';
        memcpy(output + *length, start, size);
        *length += size;
        output[*length] = '\0';
    }
    return 0;
}

int xila_path_normalize(const char *base, const char *path, char *output, int allow_escape) {
    size_t length = 1;
    output[0] = '/';
    output[1] = '\0';
    int error = append(output, &length, base, 1);
    if (error == 0)
        error = append(output, &length, path, allow_escape);
    return error;
}

int xila_path_resolve(int dirfd, const char *path, char *buffer, struct xila_path *output) {
    if (path == NULL)
        return xila_fail_errno(EFAULT);
    if (path[0] == '\0')
        return xila_fail_errno(ENOENT);
    const XilaDirectory *directory;
    const char *base;
    if (path[0] == '/') {
        directory = root();
        base = "/";
    } else if (dirfd == AT_FDCWD) {
        directory = root();
        base = xila_cwd();
    } else {
        struct xila_fd *entry = xila_fd_get(dirfd);
        if (entry == NULL)
            return -1;
        if (entry->kind != XILA_FD_DIRECTORY)
            return xila_fail_errno(ENOTDIR);
        while (path[0] == '.' && path[1] == '/') {
            path += 2;
            while (*path == '/')
                ++path;
        }
        if (path[0] == '.' && path[1] == '\0')
            ++path;
        size_t length = strlen(path);
        if (length >= XILA_PATH_MAX)
            return xila_fail_errno(ENAMETOOLONG);
        output->directory = entry->object;
        output->relative = path;
        output->length = length;
        return 0;
    }
    if (directory == NULL)
        return xila_fail_errno(EACCES); /* the host granted no filesystem capability */
    int error = xila_path_normalize(base, path, buffer, 1);
    if (error != 0)
        return xila_fail_errno(error);
    output->directory = directory;
    output->relative = buffer + 1;
    output->length = strlen(buffer + 1);
    return 0;
}
