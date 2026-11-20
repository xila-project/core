/* The ABI owns the environment. Cache only strings returned by getenv, since
 * the ABI copies values into caller-owned storage. Keep one value per name. */
#include <stdlib.h>
#include <string.h>
#include <sys/lock.h>

#include "internal.h"

struct env_value {
    struct env_value *next;
    size_t length;
    char value[]; /* NUL-terminated value, then NUL-terminated name */
};

static struct env_value *values;

static const char *value_name(const struct env_value *entry) {
    return entry->value + entry->length + 1;
}

static struct env_value **find_value(const char *name) {
    struct env_value **link = &values;
    while (*link != NULL && strcmp(value_name(*link), name) != 0)
        link = &(*link)->next;
    return link;
}

static void release_value(struct env_value **link) {
    struct env_value *entry = *link;
    if (entry != NULL) {
        *link = entry->next;
        xila_heap_release(entry);
    }
}

static struct env_value *allocate_value(size_t length, size_t name_length) {
    if (length > SIZE_MAX - sizeof(struct env_value) - name_length - 2) {
        errno = ENOMEM;
        return NULL;
    }
    return xila_heap_alloc(sizeof(struct env_value) + length + name_length + 2);
}

static char *get_value(const char *name) {
    char small[64];
    char *buffer = small;
    size_t capacity = sizeof(small);
    struct env_value **link = find_value(name);
    struct env_value *cached = *link;
    size_t name_length = strlen(name);
    size_t length;
    struct env_value *pending = NULL;
    XilaStatus status;

    for (;;) {
        status = xila_env_get(xila_process.process, name, name_length, buffer, capacity, &length);
        if (status != XILA_STATUS_BUFFER_TOO_SMALL)
            break;
        xila_heap_release(pending);
        pending = allocate_value(length, name_length);
        if (pending == NULL)
            return NULL;
        buffer = pending->value;
        capacity = length;
    }

    if (status != XILA_STATUS_OK) {
        xila_heap_release(pending);
        if (status == XILA_STATUS_NOT_FOUND)
            release_value(link);
        else
            xila_fail(status);
        return NULL;
    }

    if (cached != NULL && cached->length == length && memcmp(cached->value, buffer, length) == 0) {
        xila_heap_release(pending);
        return cached->value;
    }

    if (pending == NULL) {
        pending = allocate_value(length, name_length);
        if (pending == NULL)
            return NULL;
        memcpy(pending->value, buffer, length);
    }
    pending->length = length;
    pending->value[length] = '\0';
    memcpy(pending->value + length + 1, name, name_length + 1);
    release_value(link);
    pending->next = *link;
    *link = pending;
    return pending->value;
}

int xila_posix_env_commit(const char *name, size_t name_length, const char *value) {
    XilaStatus status =
        value != NULL ? xila_env_set(xila_process.process, name, name_length, value, strlen(value))
                      : xila_env_remove(xila_process.process, name, name_length);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    release_value(find_value(name));
    return 0;
}

/* The optional environ member overrides this hook when linked. Direct calls
 * to the hidden default use no GOT slot in an accessor-only image. */
__attribute__((weak, visibility("hidden"), noinline)) int
xila_environ_change(const char *name, size_t name_length, const char *value) {
    return xila_posix_env_commit(name, name_length, value);
}

/* Keep the opt-in table and ownership handling in environ.o. The old SDK
 * table can contain an argument to this call, so release it after the call. */
__attribute__((weak, visibility("hidden"), noinline)) int xila_environ_sync(char ***previous) {
    (void)previous;
    return 0;
}

__attribute__((weak, visibility("hidden"), noinline)) void xila_environ_release(char **table) {
    (void)table;
}

char *getenv(const char *name) {
    if (xila_process.process == NULL) {
        if (name == NULL)
            xila_fail_errno(EINVAL);
        return NULL;
    }

    __LIBC_LOCK();
    char **previous = NULL;
    char *value = NULL;
    if (xila_environ_sync(&previous) == 0) {
        if (name != NULL)
            value = get_value(name);
        else
            xila_fail_errno(EINVAL);
    }
    if (previous != NULL)
        xila_environ_release(previous);
    __LIBC_UNLOCK();
    return value;
}

static int set_value(const char *name, const char *value, int overwrite) {
    size_t name_length = strlen(name);
    if (!overwrite) {
        /* A zero-capacity read distinguishes missing, empty and nonempty. */
        size_t length;
        char unused;
        XilaStatus status =
            xila_env_get(xila_process.process, name, name_length, &unused, 0, &length);
        if (status != XILA_STATUS_NOT_FOUND)
            return status == XILA_STATUS_OK || status == XILA_STATUS_BUFFER_TOO_SMALL
                       ? 0
                       : xila_fail(status);
    }
    return xila_environ_change(name, name_length, value);
}

static int change_value(const char *name, const char *value, int overwrite, bool remove) {
    bool valid = (value != NULL || remove) && name != NULL && *name != '\0' &&
                 strchr(name, '=') == NULL;
    if (xila_process.process == NULL)
        return xila_fail_errno(valid ? ENOSYS : EINVAL);

    __LIBC_LOCK();
    char **previous = NULL;
    int result = xila_environ_sync(&previous);
    if (result == 0)
        result = valid ? set_value(name, value, overwrite) : xila_fail_errno(EINVAL);
    if (previous != NULL)
        xila_environ_release(previous);
    __LIBC_UNLOCK();
    return result;
}

int setenv(const char *name, const char *value, int overwrite) {
    return change_value(name, value, overwrite, false);
}

int unsetenv(const char *name) {
    return change_value(name, NULL, 1, true);
}
