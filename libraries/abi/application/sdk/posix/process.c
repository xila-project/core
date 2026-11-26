/* Process identity, and posix_spawn and waitpid on the ABI's process calls.
 *
 * A child gets exactly three descriptors, its standard streams, which the host duplicates
 * from the files given at the spawn: file actions edit that three-slot view. Spawned
 * children are kept in a small heap list of {pid, handle} that waitpid searches; the host
 * closes the handles left in it when the process ends. */
#include <fcntl.h>
#include <spawn.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include "internal.h"

pid_t getpid(void) {
    if (xila_process.process == NULL)
        return 1;
    uint32_t id;
    XilaStatus status = xila_process_id(xila_process.process, &id);
    return status == XILA_STATUS_OK ? (pid_t)id : xila_fail(status);
}

pid_t getppid(void) {
    if (xila_process.process == NULL)
        return 0;
    uint32_t id;
    XilaStatus status = xila_process_parent_id(xila_process.process, &id);
    return status == XILA_STATUS_OK ? (pid_t)id : xila_fail(status);
}

/* ---- File actions ---- */

enum { ACTION_OPEN = 1, ACTION_CLOSE, ACTION_DUP2 };

struct spawn_action {
    uint8_t kind;
    int fd;
    int source;  /* ACTION_DUP2 */
    int flags;   /* ACTION_OPEN */
    char *path;  /* ACTION_OPEN, owned */
};

struct __posix_spawn_file_actions {
    struct spawn_action *actions;
    size_t count;
    size_t capacity;
};

int posix_spawn_file_actions_init(posix_spawn_file_actions_t *actions) {
    *actions = xila_heap_alloc(sizeof(**actions));
    if (*actions == NULL)
        return ENOMEM;
    memset(*actions, 0, sizeof(**actions));
    return 0;
}

int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *actions) {
    if (*actions == NULL)
        return EINVAL;
    for (size_t index = 0; index < (*actions)->count; ++index)
        xila_heap_release((*actions)->actions[index].path);
    xila_heap_release((*actions)->actions);
    xila_heap_release(*actions);
    *actions = NULL;
    return 0;
}

static int add_action(posix_spawn_file_actions_t *actions, struct spawn_action action) {
    if (actions == NULL || *actions == NULL)
        return EINVAL;
    if (action.fd < 0 || action.fd >= XILA_OPEN_MAX ||
        (action.kind == ACTION_DUP2 && (action.source < 0 || action.source >= XILA_OPEN_MAX)))
        return EBADF;
    struct __posix_spawn_file_actions *list = *actions;
    if (list->count == list->capacity) {
        size_t capacity = list->capacity == 0 ? 4 : list->capacity * 2;
        void *grown = NULL;
        if (xila_heap_reallocate(xila_process.startup->heap, list->actions,
                                 capacity * sizeof(*list->actions), &grown) != XILA_STATUS_OK)
            return ENOMEM;
        list->actions = grown;
        list->capacity = capacity;
    }
    list->actions[list->count++] = action;
    return 0;
}

int posix_spawn_file_actions_addopen(posix_spawn_file_actions_t *actions, int fd,
                                     const char *path, int flags, mode_t mode) {
    (void)mode; /* Mode bits are ignored, as for open. */
    if (path == NULL)
        return EINVAL;
    size_t length = strlen(path) + 1;
    char *copy = xila_heap_alloc(length);
    if (copy == NULL)
        return ENOMEM;
    memcpy(copy, path, length);
    int error = add_action(actions, (struct spawn_action){.kind = ACTION_OPEN, .fd = fd,
                                                          .flags = flags, .path = copy});
    if (error != 0)
        xila_heap_release(copy);
    return error;
}

int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *actions, int fd) {
    return add_action(actions, (struct spawn_action){.kind = ACTION_CLOSE, .fd = fd});
}

int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *actions, int fd, int target) {
    return add_action(actions,
                      (struct spawn_action){.kind = ACTION_DUP2, .fd = target, .source = fd});
}

/* ---- Attributes ---- */

#define SUPPORTED_FLAGS (POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSIGDEF)

struct __posix_spawnattr {
    short flags;
    pid_t process_group;
    sigset_t signal_mask;
    sigset_t signal_default;
};

int posix_spawnattr_init(posix_spawnattr_t *attributes) {
    *attributes = xila_heap_alloc(sizeof(**attributes));
    if (*attributes == NULL)
        return ENOMEM;
    memset(*attributes, 0, sizeof(**attributes));
    return 0;
}

int posix_spawnattr_destroy(posix_spawnattr_t *attributes) {
    if (*attributes == NULL)
        return EINVAL;
    xila_heap_release(*attributes);
    *attributes = NULL;
    return 0;
}

int posix_spawnattr_setflags(posix_spawnattr_t *attributes, short flags) {
    const short known = POSIX_SPAWN_RESETIDS | POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_SETSCHEDPARAM |
                        POSIX_SPAWN_SETSCHEDULER | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK;
    if (flags & ~known)
        return EINVAL;
    (*attributes)->flags = flags;
    return 0;
}

int posix_spawnattr_getflags(const posix_spawnattr_t *attributes, short *flags) {
    *flags = (*attributes)->flags;
    return 0;
}

int posix_spawnattr_setpgroup(posix_spawnattr_t *attributes, pid_t group) {
    (*attributes)->process_group = group;
    return 0;
}

int posix_spawnattr_getpgroup(const posix_spawnattr_t *attributes, pid_t *group) {
    *group = (*attributes)->process_group;
    return 0;
}

int posix_spawnattr_setsigmask(posix_spawnattr_t *attributes, const sigset_t *mask) {
    (*attributes)->signal_mask = *mask;
    return 0;
}

int posix_spawnattr_getsigmask(const posix_spawnattr_t *attributes, sigset_t *mask) {
    *mask = (*attributes)->signal_mask;
    return 0;
}

int posix_spawnattr_setsigdefault(posix_spawnattr_t *attributes, const sigset_t *signals) {
    (*attributes)->signal_default = *signals;
    return 0;
}

int posix_spawnattr_getsigdefault(const posix_spawnattr_t *attributes, sigset_t *signals) {
    *signals = (*attributes)->signal_default;
    return 0;
}

/* ---- Children ---- */

struct spawned_child {
    pid_t pid;
    XilaChild *handle;
    bool waiting; /* A waitpid is suspended on it. */
};

static struct spawned_child *children;
static size_t child_count;
static size_t child_capacity;

/* Back to the pristine empty list, for a relaunch of the same linked program after a
   previous run was cancelled: the list was allocated on that run's heap, which the host
   freed, and the host closed the child handles left in it. startup.c calls this hook
   weakly, so an image that never spawns does not link it. */
__attribute__((visibility("hidden"))) void xila_process_reset(void) {
    children = NULL;
    child_count = 0;
    child_capacity = 0;
}

static int remember_child(pid_t pid, XilaChild *handle) {
    if (child_count == child_capacity) {
        size_t capacity = child_capacity == 0 ? 4 : child_capacity * 2;
        void *grown = NULL;
        if (xila_heap_reallocate(xila_process.startup->heap, children,
                                 capacity * sizeof(*children), &grown) != XILA_STATUS_OK)
            return ENOMEM;
        children = grown;
        child_capacity = capacity;
    }
    children[child_count++] = (struct spawned_child){pid, handle, false};
    return 0;
}

static void forget_child(XilaChild *handle) {
    for (size_t index = 0; index < child_count; ++index) {
        if (children[index].handle == handle) {
            children[index] = children[--child_count];
            break;
        }
    }
    (void)xila_process_child_close(handle);
}

/* ---- posix_spawn ---- */

/* One of the child's three standard streams. `owned` is a parent descriptor opened by a
   file action, closed once the host has duplicated it. */
struct stream_view {
    XilaFile *file;
    int owned;
};

static void close_owned(struct stream_view view[3]) {
    for (int slot = 0; slot < 3; ++slot) {
        if (view[slot].owned < 0)
            continue;
        int still_used = 0;
        for (int other = slot + 1; other < 3; ++other)
            still_used |= view[other].owned == view[slot].owned;
        if (!still_used)
            (void)close(view[slot].owned);
        view[slot].owned = -1;
    }
}

/* Give a slot a new stream, closing a descriptor opened by an earlier file action once no
   slot uses it any more. */
static void replace_slot(struct stream_view view[3], int slot, struct stream_view next) {
    int owned = view[slot].owned;
    view[slot] = next;
    if (owned < 0)
        return;
    for (int other = 0; other < 3; ++other)
        if (view[other].owned == owned)
            return;
    (void)close(owned);
}

static int parent_file(int fd, XilaFile **file) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL || entry->kind != XILA_FD_FILE)
        return EBADF;
    *file = entry->object;
    return 0;
}

/* Build the child's three streams: the parent's 0-2, then the file actions in order. */
static int stream_views(const posix_spawn_file_actions_t *actions, struct stream_view view[3]) {
    for (int slot = 0; slot < 3; ++slot) {
        struct xila_fd *entry = xila_fd_get(slot);
        int inherited = entry != NULL && entry->kind == XILA_FD_FILE &&
                        !(entry->flags & XILA_FD_CLOEXEC);
        view[slot] = (struct stream_view){inherited ? entry->object : NULL, -1};
    }
    if (actions == NULL || *actions == NULL)
        return 0;
    const struct __posix_spawn_file_actions *list = *actions;
    for (size_t index = 0; index < list->count; ++index) {
        const struct spawn_action *action = &list->actions[index];
        if (action->kind == ACTION_CLOSE) {
            /* The child inherits no descriptor above 2, so closing one there is done. */
            if (action->fd < 3)
                replace_slot(view, action->fd, (struct stream_view){NULL, -1});
            continue;
        }
        /* A child cannot receive a descriptor above 2. */
        if (action->fd >= 3)
            return ENOTSUP;
        if (action->kind == ACTION_DUP2) {
            struct stream_view source = {NULL, -1};
            if (action->source < 3) {
                source = view[action->source];
            } else {
                int error = parent_file(action->source, &source.file);
                if (error != 0)
                    return error;
            }
            if (source.file == NULL)
                return EBADF;
            replace_slot(view, action->fd, source);
            continue;
        }
        int fd = open(action->path, action->flags, 0);
        if (fd < 0)
            return errno;
        XilaFile *file = NULL;
        int error = parent_file(fd, &file);
        if (error != 0) {
            (void)close(fd);
            return EISDIR;
        }
        replace_slot(view, action->fd, (struct stream_view){file, fd});
    }
    return 0;
}

/* NUL-terminated strings as ABI strings, in one heap array. */
static int string_array(char *const strings[], size_t skip, XilaString **output, size_t *count) {
    size_t total = 0;
    while (strings != NULL && strings[total] != NULL)
        ++total;
    *output = NULL;
    *count = total > skip ? total - skip : 0;
    if (*count == 0)
        return 0;
    *output = xila_heap_alloc(*count * sizeof(**output));
    if (*output == NULL)
        return ENOMEM;
    for (size_t index = 0; index < *count; ++index)
        (*output)[index] = (XilaString){strings[skip + index], strlen(strings[skip + index])};
    return 0;
}

static int spawn_resolved(pid_t *pid, const char *path,
                          const posix_spawn_file_actions_t *actions,
                          const posix_spawnattr_t *attributes, char *const arguments[],
                          char *const environment[]) {
    if (xila_process.process == NULL)
        return ENOSYS;
    if (attributes != NULL && *attributes != NULL && ((*attributes)->flags & ~SUPPORTED_FLAGS))
        return ENOTSUP;
    char buffer[XILA_PATH_MAX];
    struct xila_path resolved;
    if (xila_path_resolve(AT_FDCWD, path, buffer, &resolved) != 0)
        return errno;
    struct stream_view view[3];
    int error = stream_views(actions, view);
    if (error == 0 && (view[0].file == NULL || view[1].file == NULL || view[2].file == NULL))
        error = EBADF; /* The host needs all three standard streams. */
    XilaSpawn spawn;
    memset(&spawn, 0, sizeof(spawn));
    XilaString *argument_strings = NULL, *environment_strings = NULL;
    if (error == 0)
        /* argv[0] is the program name, which the host derives from the path itself. */
        error = string_array(arguments, 1, &argument_strings, &spawn.argument_count);
    if (error == 0 && environment != NULL) {
        error = string_array(environment, 0, &environment_strings, &spawn.environment_count);
        /* A non-null array, empty or not, is the child's whole environment. */
        static const XilaString empty_environment[1];
        spawn.environment = environment_strings != NULL ? environment_strings : empty_environment;
    }
    XilaChild *child = NULL;
    if (error == 0) {
        spawn.size = sizeof(spawn);
        spawn.arguments = argument_strings;
        spawn.standard_input = view[0].file;
        spawn.standard_output = view[1].file;
        spawn.standard_error = view[2].file;
        spawn.home = xila_process.startup->home;
        XilaStatus status = xila_process_spawn(xila_process.process, resolved.directory,
                                               resolved.relative, resolved.length, &spawn, &child);
        if (status != XILA_STATUS_OK)
            error = xila_errno_from_status(status);
    }
    close_owned(view);
    xila_heap_release(argument_strings);
    xila_heap_release(environment_strings);
    if (error != 0)
        return error;
    uint32_t id = 0;
    XilaStatus status = xila_process_child_id(child, &id);
    if (status == XILA_STATUS_OK)
        error = remember_child((pid_t)id, child);
    else
        error = xila_errno_from_status(status);
    if (error != 0) {
        /* The child runs on, detached; it cannot be waited for. */
        (void)xila_process_child_close(child);
        return error;
    }
    if (pid != NULL)
        *pid = (pid_t)id;
    return 0;
}

int posix_spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *actions,
                const posix_spawnattr_t *attributes, char *const arguments[],
                char *const environment[]) {
    int saved = errno;
    int error = spawn_resolved(pid, path, actions, attributes, arguments, environment);
    errno = saved;
    return error;
}

int posix_spawnp(pid_t *pid, const char *file, const posix_spawn_file_actions_t *actions,
                 const posix_spawnattr_t *attributes, char *const arguments[],
                 char *const environment[]) {
    if (file == NULL || file[0] == '\0' || strchr(file, '/') != NULL)
        return posix_spawn(pid, file, actions, attributes, arguments, environment);
    int saved = errno;
    const char *search = getenv("PATH");
    /* There is no default search path: without PATH, only a path with a slash runs. */
    int error = ENOENT;
    size_t file_length = strlen(file);
    while (search != NULL) {
        const char *end = strchr(search, ':');
        size_t length = end == NULL ? strlen(search) : (size_t)(end - search);
        char candidate[XILA_PATH_MAX];
        /* An empty entry is the current directory. */
        const char *directory = length == 0 ? "." : search;
        size_t directory_length = length == 0 ? 1 : length;
        if (directory_length + 1 + file_length < sizeof(candidate)) {
            memcpy(candidate, directory, directory_length);
            candidate[directory_length] = '/';
            memcpy(candidate + directory_length + 1, file, file_length + 1);
            int attempt = spawn_resolved(pid, candidate, actions, attributes, arguments,
                                         environment);
            /* Keep searching only while the program is not found at this entry. */
            if (attempt != ENOENT && attempt != ENOTDIR && attempt != EACCES) {
                error = attempt;
                break;
            }
            if (attempt == EACCES || error == ENOENT)
                error = attempt;
        }
        search = end == NULL ? NULL : end + 1;
    }
    errno = saved;
    return error;
}

/* ---- waitpid ---- */

/* Xila exit statuses are 32-bit and there are no signals: every child exits normally,
   with the low eight bits of its status, as on POSIX systems. */
static int encode_status(int32_t status) { return ((int)status & 0xff) << 8; }

/* Collect one child. Returns 1 when it was reaped, 0 when it is still running (no-hang),
   or -1 with errno set. */
static int collect(size_t index, uint32_t flags, int *status) {
    XilaChild *handle = children[index].handle;
    children[index].waiting = true;
    int32_t exit_status = 0;
    XilaStatus result = xila_process_wait(xila_process.process, handle, flags, &exit_status);
    /* The list may have moved while the wait was suspended: find the entry again. */
    for (index = 0; index < child_count && children[index].handle != handle; ++index)
        ;
    if (index < child_count)
        children[index].waiting = false;
    if (result == XILA_STATUS_WOULD_BLOCK)
        return 0;
    if (result != XILA_STATUS_OK && result != XILA_STATUS_NO_CHILD)
        return xila_fail(result);
    forget_child(handle);
    if (result == XILA_STATUS_NO_CHILD)
        return xila_fail_errno(ECHILD);
    if (status != NULL)
        *status = encode_status(exit_status);
    return 1;
}

pid_t waitpid(pid_t pid, int *status, int options) {
    if (options & ~(WNOHANG | WUNTRACED | WCONTINUED))
        return xila_fail_errno(EINVAL);
    if (xila_process.process == NULL)
        return xila_fail_errno(ECHILD);
    /* Every child is in the caller's process group, whose identifier is the caller's. */
    int any = pid == -1 || pid == 0 || (pid < -1 && -pid == getpid());
    if (pid < -1 && !any)
        return xila_fail_errno(ECHILD);
    /* The ABI waits for one handle: a single candidate is waited for directly, and several
       are polled, sleeping between rounds from 1 ms up to 16 ms. A blocking wait the host
       cannot suspend (outside a coroutine) is polled too, never reported as done. */
    uint64_t pause = 1000000;
    for (;;) {
        size_t candidates = 0, last = 0;
        for (size_t index = 0; index < child_count; ++index) {
            if (!any && children[index].pid != pid)
                continue;
            if (children[index].waiting) {
                if (!any)
                    return xila_fail_errno(ECHILD); /* Another thread is collecting it. */
                continue;
            }
            ++candidates;
            last = index;
        }
        if (candidates == 0)
            return xila_fail_errno(ECHILD);
        if (candidates == 1 && !(options & WNOHANG)) {
            pid_t found = children[last].pid;
            int reaped = collect(last, 0, status);
            if (reaped != 0)
                return reaped < 0 ? -1 : found;
        } else {
            for (size_t index = 0; index < child_count; ++index) {
                if ((!any && children[index].pid != pid) || children[index].waiting)
                    continue;
                pid_t found = children[index].pid;
                int reaped = collect(index, XILA_WAIT_NO_HANG, status);
                if (reaped != 0)
                    return reaped < 0 ? -1 : found;
            }
            if (options & WNOHANG)
                return 0;
        }
        XilaStatus slept = xila_time_sleep(pause);
        if (slept != XILA_STATUS_OK)
            return xila_fail(slept);
        if (pause < 16000000)
            pause *= 2;
    }
}

pid_t wait(int *status) { return waitpid(-1, status, 0); }
