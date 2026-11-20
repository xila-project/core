/* Optional POSIX view of the launch environment and subsequent POSIX changes.
 * A strong reference to environ extracts this member from libxila_posix.a.
 * Startup and the mutation paths supply weak defaults for its hooks. */
#include <stdlib.h>
#include <string.h>
#include <xila/posix.h>

#include "internal.h"

static char *empty_environment[1];
char **environ = empty_environment;

__attribute__((visibility("hidden"))) int xila_environ_initialize(void) {
    if (xila_process.process == NULL)
        return 0;

    for (size_t index = 0;; ++index) {
        size_t length;
        XilaStatus status = xila_env_entry(xila_process.process, index, NULL, 0, &length);
        if (status == XILA_STATUS_NOT_FOUND)
            return 0;
        if (status != XILA_STATUS_BUFFER_TOO_SMALL)
            return xila_fail(status);

        char *entry = xila_heap_alloc(length + 1);
        if (entry == NULL)
            return -1;
        status = xila_env_entry(xila_process.process, index, entry, length, &length);
        if (status != XILA_STATUS_OK) {
            xila_heap_release(entry);
            return xila_fail(status);
        }
        entry[length] = '\0';

        char **table = reallocarray(index == 0 ? NULL : environ, index + 2, sizeof(*table));
        if (table == NULL) {
            xila_heap_release(entry);
            return -1;
        }
        table[index] = entry;
        table[index + 1] = NULL;
        environ = table;
    }
}

static bool matches(const char *entry, const char *name, size_t name_length) {
    return strncmp(entry, name, name_length) == 0 && entry[name_length] == '=';
}

__attribute__((visibility("hidden"))) int xila_environ_change(const char *name, size_t name_length,
                                                              const char *value) {
    size_t count = 0;
    char *previous = NULL;
    for (; environ[count] != NULL; ++count)
        if (previous == NULL && matches(environ[count], name, name_length))
            previous = environ[count];

    /* Prepare replacements before committing so ENOMEM leaves both views intact. */
    char *entry = NULL;
    char **extended = NULL;
    if (value != NULL) {
        size_t value_bytes = strlen(value) + 1;
        size_t bytes;
        if (__builtin_add_overflow(name_length + 1, value_bytes, &bytes))
            return xila_fail_errno(ENOMEM);
        entry = xila_heap_alloc(bytes);
        if (entry == NULL)
            return -1;
        memcpy(entry, name, name_length);
        entry[name_length] = '=';
        memcpy(entry + name_length + 1, value, value_bytes);

        if (previous == NULL) {
            extended = xila_heap_alloc((count + 2) * sizeof(*extended));
            if (extended == NULL) {
                xila_heap_release(entry);
                return -1;
            }
            memcpy(extended, environ, count * sizeof(*extended));
            extended[count] = entry;
            extended[count + 1] = NULL;
        }
    }

    if (xila_posix_env_commit(name, name_length, value) != 0) {
        xila_heap_release(extended);
        xila_heap_release(entry);
        return -1;
    }
    if (extended != NULL) {
        if (count != 0)
            xila_heap_release(environ);
        environ = extended;
    } else if (previous != NULL) {
        /* The host removes every occurrence. Keep the old name alive until we
         * have scanned the remaining entries, even if name aliased the cache. */
        size_t remaining = 0;
        for (size_t index = 0; index < count; ++index) {
            char *current = environ[index];
            if (current == previous) {
                if (entry != NULL)
                    environ[remaining++] = entry;
            } else if (matches(current, previous, name_length)) {
                xila_heap_release(current);
            } else {
                environ[remaining++] = current;
            }
        }
        xila_heap_release(previous);
        environ[remaining] = NULL;
        if (remaining == 0) {
            xila_heap_release(environ);
            environ = empty_environment;
        }
    }
    return 0;
}
