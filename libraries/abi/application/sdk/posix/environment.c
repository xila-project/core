/* The ABI owns the environment. Cache only strings returned by getenv, since
 * the ABI copies values into caller-owned storage and raw ABI updates can
 * happen between POSIX calls. */
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
        if (status != XILA_STATUS_NOT_FOUND)
            xila_fail(status);
        return NULL;
    }

    for (struct env_value *entry = values; entry != NULL; entry = entry->next) {
        if (entry->length == length && strcmp(value_name(entry), name) == 0 &&
            memcmp(entry->value, buffer, length) == 0) {
            xila_heap_release(pending);
            return entry->value;
        }
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
    pending->next = values;
    values = pending;
    return pending->value;
}

/* A successful POSIX mutation invalidates this name's snapshots. Defer freeing
 * them until the scan is finished: name itself can point into one of them. */
static void forget_values(const char *name) {
    struct env_value *removed = NULL;
    struct env_value **link = &values;
    while (*link != NULL) {
        struct env_value *entry = *link;
        if (strcmp(value_name(entry), name) == 0) {
            *link = entry->next;
            entry->next = removed;
            removed = entry;
        } else {
            link = &entry->next;
        }
    }
    while (removed != NULL) {
        struct env_value *entry = removed;
        removed = entry->next;
        xila_heap_release(entry);
    }
}

char *getenv(const char *name) {
    if (xila_process.process == NULL)
        return NULL;
    __LIBC_LOCK();
    char *value = get_value(name);
    __LIBC_UNLOCK();
    return value;
}

int setenv(const char *name, const char *value, int overwrite) {
    if (xila_process.process == NULL)
        return xila_fail_errno(ENOSYS);

    size_t name_length = strlen(name);
    __LIBC_LOCK();
    if (!overwrite) {
        /* A zero-capacity read distinguishes missing, empty and nonempty. */
        size_t length;
        char unused;
        XilaStatus status =
            xila_env_get(xila_process.process, name, name_length, &unused, 0, &length);
        if (status != XILA_STATUS_NOT_FOUND) {
            __LIBC_UNLOCK();
            return status == XILA_STATUS_OK || status == XILA_STATUS_BUFFER_TOO_SMALL
                       ? 0
                       : xila_fail(status);
        }
    }
    XilaStatus status = xila_env_set(xila_process.process, name, name_length, value, strlen(value));
    if (status == XILA_STATUS_OK)
        forget_values(name);
    __LIBC_UNLOCK();
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

int unsetenv(const char *name) {
    if (xila_process.process == NULL)
        return xila_fail_errno(ENOSYS);

    __LIBC_LOCK();
    XilaStatus status = xila_env_remove(xila_process.process, name, strlen(name));
    if (status == XILA_STATUS_OK)
        forget_values(name);
    __LIBC_UNLOCK();
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}
