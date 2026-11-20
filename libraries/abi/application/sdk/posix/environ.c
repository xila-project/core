/* Optional POSIX view of the launch environment and subsequent POSIX changes.
 * A strong reference to environ extracts this member from libxila_posix.a.
 * Startup and the accessors supply weak defaults for its hooks. */
#include <stdlib.h>
#include <string.h>
#include <xila/posix.h>

#include "internal.h"

static char *empty_environment[1];
static char **owned = empty_environment;
static char **retained;
char **environ = empty_environment;

static void publish(char **table) {
    owned = environ = table != NULL ? table : empty_environment;
}

__attribute__((visibility("hidden"))) void xila_environ_release(char **table) {
    if (table == NULL || table == empty_environment)
        return;
    for (size_t index = 0; table[index] != NULL; ++index)
        xila_heap_release(table[index]);
    xila_heap_release(table);
}

/* A host entry can grow between calls. Retry with the new length, just as
 * getenv does. A vanished index marks the end of the current enumeration. */
static int read_entry(size_t index, char **entry) {
    size_t capacity = 0;
    size_t length;
    XilaStatus status;
    *entry = NULL;
    while ((status = xila_env_entry(xila_process.process, index, *entry, capacity, &length)) ==
           XILA_STATUS_BUFFER_TOO_SMALL) {
        xila_heap_release(*entry);
        *entry = xila_heap_alloc(length + 1);
        if (*entry == NULL)
            return -1;
        capacity = length;
    }
    if (status == XILA_STATUS_OK) {
        (*entry)[length] = '\0';
        return 1;
    }
    xila_heap_release(*entry);
    return status == XILA_STATUS_NOT_FOUND ? 0 : xila_fail(status);
}

/* Enumerate once, growing only at powers of two. The partial table is always
 * terminated so startup and adoption can release it if a later read fails. */
static int read_host(char ***output) {
    char **table = NULL;
    for (size_t index = 0;; ++index) {
        char *entry;
        int result = read_entry(index, &entry);
        if (result == 0) {
            *output = table;
            return 0;
        }
        if (result < 0)
            break;
        size_t count = index + 1;
        if ((count & (count - 1)) == 0) {
            char **larger = reallocarray(table, count, 2 * sizeof(*larger));
            if (larger == NULL) {
                xila_heap_release(entry);
                break;
            }
            table = larger;
        }
        table[index] = entry;
        table[count] = NULL;
    }
    xila_environ_release(table);
    return -1;
}

__attribute__((visibility("hidden"))) int xila_environ_initialize(void) {
    if (xila_process.process == NULL)
        return 0;
    char **table;
    if (read_host(&table) != 0)
        return -1;
    publish(table);
    return 0;
}

static bool matches(const char *entry, const char *name, size_t name_length) {
    return strncmp(entry, name, name_length) == 0 && entry[name_length] == '=';
}

/* The host accepts UTF-8 names and values regardless of the C locale. */
static bool valid_utf8(const unsigned char *entry) {
    while (*entry != '\0') {
        unsigned char lead = *entry++;
        if (lead < 0x80)
            continue;
        if (lead < 0xc2 || lead > 0xf4)
            return false;

        unsigned int remaining = lead < 0xe0 ? 1 : lead < 0xf0 ? 2 : 3;
        /* Constrain the first continuation to exclude overlong encodings,
         * surrogates and code points above U+10FFFF. */
        unsigned char minimum = lead == 0xe0 ? 0xa0 : lead == 0xf0 ? 0x90 : 0x80;
        unsigned char maximum = lead == 0xed ? 0x9f : lead == 0xf4 ? 0x8f : 0xbf;
        do {
            unsigned char byte = *entry++;
            if (byte < minimum || byte > maximum)
                return false;
            minimum = 0x80;
            maximum = 0xbf;
        } while (--remaining != 0);
    }
    return true;
}

static int copy_assigned(char ***output) {
    size_t count = 0;
    if (environ != NULL)
        while (environ[count] != NULL)
            ++count;
    *output = NULL;
    if (count == 0)
        return 0;

    char **table = reallocarray(NULL, count + 1, sizeof(*table));
    if (table == NULL)
        return -1;
    size_t used = 0;
    table[0] = NULL;
    for (size_t index = 0; index < count; ++index) {
        const char *entry = environ[index];
        const char *equals = strchr(entry, '=');
        if (equals == NULL || equals == entry || !valid_utf8((const unsigned char *)entry))
            continue;
        /* getenv on a supplied array would read the first match. */
        size_t match = 0;
        while (match < used && !matches(table[match], entry, equals - entry))
            ++match;
        if (match != used)
            continue;

        char *copy = strdup(entry);
        if (copy == NULL) {
            xila_environ_release(table);
            return -1;
        }
        table[used++] = copy;
        table[used] = NULL;
    }
    if (used == 0)
        xila_heap_release(table);
    else
        *output = table;
    return 0;
}

/* Use the raw ABI here so cached getenv pointers survive until the accessor
 * observes a change. A caller may be using one as this call's argument. */
static XilaStatus write_host(char **table, bool remove) {
    if (table == NULL)
        return XILA_STATUS_OK;
    for (size_t index = 0; table[index] != NULL; ++index) {
        const char *entry = table[index];
        const char *equals = strchr(entry, '=');
        size_t length = equals - entry;
        XilaStatus status = remove
            ? xila_env_remove(xila_process.process, entry, length)
            : xila_env_set(xila_process.process, entry, length, equals + 1, strlen(equals + 1));
        if (status != XILA_STATUS_OK)
            return status;
    }
    return XILA_STATUS_OK;
}

__attribute__((visibility("hidden"))) int xila_environ_sync(char ***evicted) {
    if (environ == owned)
        return 0;

    bool restoring = environ != NULL && environ == retained;
    char **replacement = retained;
    if (!restoring && copy_assigned(&replacement) != 0)
        return -1;
    /* Include raw ABI changes, which need not be reflected in the owned table.
     * Finish all guest allocations before the first host mutation. */
    char **host;
    if (read_host(&host) != 0) {
        if (!restoring)
            xila_environ_release(replacement);
        return -1;
    }
    XilaStatus status = write_host(host, true);
    if (status == XILA_STATUS_OK) {
        status = write_host(replacement, false);
        if (status != XILA_STATUS_OK) {
            /* Remove any successfully installed prefix before restoring the
             * snapshot. No guest allocations are needed for recovery. */
            write_host(replacement, true);
        }
    }
    if (status != XILA_STATUS_OK) {
        write_host(host, false);
        xila_environ_release(host);
        if (!restoring)
            xila_environ_release(replacement);
        return xila_fail(status);
    }
    xila_environ_release(host);
    /* Keep one previous SDK table available for save/assign/restore. A
     * restore swaps the two SDK tables without copying either. */
    if (!restoring)
        *evicted = retained;
    retained = owned;
    publish(replacement);
    return 0;
}

__attribute__((visibility("hidden"))) int xila_environ_change(const char *name, size_t name_length,
                                                              const char *value) {
    size_t count = 0;
    char *previous = NULL;
    for (; owned[count] != NULL; ++count)
        if (previous == NULL && matches(owned[count], name, name_length))
            previous = owned[count];

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
            memcpy(extended, owned, count * sizeof(*extended));
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
        if (owned != empty_environment)
            xila_heap_release(owned);
        publish(extended);
    } else if (previous != NULL) {
        /* The host removes every occurrence. Keep the old name alive until we
         * have scanned the remaining entries, even if name aliased the cache. */
        size_t remaining = 0;
        for (size_t index = 0; index < count; ++index) {
            char *current = owned[index];
            if (current == previous) {
                if (entry != NULL)
                    owned[remaining++] = entry;
            } else if (matches(current, previous, name_length)) {
                xila_heap_release(current);
            } else {
                owned[remaining++] = current;
            }
        }
        xila_heap_release(previous);
        owned[remaining] = NULL;
    }
    return 0;
}
