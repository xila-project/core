/* Thread declarations. Creating a thread fails with ENOSYS until the native ABI exposes
 * threads, coroutine-local storage and synchronization (core#718). */
#ifndef _XILA_PTHREAD_H
#define _XILA_PTHREAD_H

#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned long pthread_t;
typedef struct {
    unsigned long stack_size;
} pthread_attr_t;

int pthread_create(pthread_t *thread, const pthread_attr_t *attributes, void *(*entry)(void *),
                   void *argument);
int pthread_join(pthread_t thread, void **result);
int pthread_detach(pthread_t thread);

#ifdef __cplusplus
}
#endif

#endif
