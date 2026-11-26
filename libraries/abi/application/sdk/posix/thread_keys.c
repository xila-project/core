#include <pthread.h>
#include <string.h>

#include "internal.h"

struct key_destructor {
    pthread_key_t key;
    void (*call)(void *);
};

/* Only linked into images which use POSIX keys. Entries with no destructor
   do not need a record here. The ABI owns key lifetimes and TLS values. */
static struct key_destructor destructors[XILA_THREAD_KEYS_MAX];

/* Back to the pristine empty table, for a relaunch of the same linked program after a
   previous run was cancelled: the ABI owns key lifetimes, and the host discarded them with
   the run. startup.c calls this hook through a weak no-op default, so an image that uses no keys does not link
   it. */
__attribute__((visibility("hidden"))) void xila_thread_keys_reset(void) {
    memset(destructors, 0, sizeof destructors);
}

int pthread_key_create(pthread_key_t *key, void (*destructor)(void *)) {
    XilaStatus status = xila_thread_key_create(key);
    if (status == XILA_STATUS_OUT_OF_MEMORY)
        return EAGAIN;
    if (status == XILA_STATUS_OK && destructor != NULL) {
        for (size_t index = 0; index < XILA_THREAD_KEYS_MAX; ++index) {
            if (destructors[index].call == NULL) {
                destructors[index] = (struct key_destructor){*key, destructor};
                break;
            }
        }
    }
    return xila_errno_from_status(status);
}

int pthread_key_delete(pthread_key_t key) {
    XilaStatus status = xila_thread_key_delete(key);
    if (status == XILA_STATUS_OK) {
        for (size_t index = 0; index < XILA_THREAD_KEYS_MAX; ++index)
            if (destructors[index].key == key) {
                destructors[index] = (struct key_destructor){0, NULL};
                break;
            }
    }
    return xila_errno_from_status(status);
}

void *pthread_getspecific(pthread_key_t key) {
    void *value = NULL;
    (void)xila_thread_tls_get(key, &value);
    return value;
}

int pthread_setspecific(pthread_key_t key, const void *value) {
    return xila_errno_from_status(xila_thread_tls_set(key, (void *)value));
}

void xila_run_key_destructors(void) {
    for (int round = 0; round < PTHREAD_DESTRUCTOR_ITERATIONS; ++round) {
        bool called = false;
        for (size_t index = 0; index < XILA_THREAD_KEYS_MAX; ++index) {
            struct key_destructor destructor = destructors[index];
            if (destructor.call == NULL)
                continue;
            void *value = pthread_getspecific(destructor.key);
            if (value != NULL) {
                (void)pthread_setspecific(destructor.key, NULL);
                destructor.call(value);
                called = true;
            }
        }
        if (!called)
            break;
    }
}
