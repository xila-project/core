#include <pthread.h>
#include <sched.h>
#include <stdlib.h>

#include "internal.h"

/* Weak hidden defaults so calls below always target a defined symbol (never a weak
   undefined function whose PLT/PC-relative call relocation a PIE host link rejects
   once resolved to the absolute 0); thread_keys.c and thread_sync.c override them
   strongly when linked. */
__attribute__((weak, visibility("hidden"), noinline)) void xila_run_key_destructors(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_abandon_once_calls(void) {}

struct thread_start {
    void *(*entry)(void *);
    void *argument;
};

static void *start_thread(void *argument) {
    struct thread_start start = *(struct thread_start *)argument;
    xila_heap_release(argument);
    void *result = start.entry(start.argument);
    xila_run_key_destructors();
    return result;
}

int pthread_attr_init(pthread_attr_t *attributes) {
    *attributes = (pthread_attr_t){0, PTHREAD_CREATE_JOINABLE};
    return 0;
}

int pthread_attr_destroy(pthread_attr_t *attributes) {
    (void)attributes;
    return 0;
}

int pthread_attr_setstacksize(pthread_attr_t *attributes, size_t size) {
    if (size < XILA_THREAD_STACK_MIN || size > XILA_THREAD_STACK_MAX)
        return EINVAL;
    attributes->stack_size = size;
    return 0;
}

int pthread_attr_getstacksize(const pthread_attr_t *attributes, size_t *size) {
#if defined(__x86_64__) || defined(__aarch64__)
    const size_t default_size = XILA_THREAD_STACK_MAX;
#else
    const size_t default_size = 4096;
#endif
    *size = attributes->stack_size != 0 ? attributes->stack_size : default_size;
    return 0;
}

int pthread_attr_setdetachstate(pthread_attr_t *attributes, int state) {
    if (state != PTHREAD_CREATE_JOINABLE && state != PTHREAD_CREATE_DETACHED)
        return EINVAL;
    attributes->detach_state = state;
    return 0;
}

int pthread_attr_getdetachstate(const pthread_attr_t *attributes, int *state) {
    *state = attributes->detach_state;
    return 0;
}

int pthread_create(pthread_t *thread, const pthread_attr_t *attributes, void *(*entry)(void *),
                   void *argument) {
    if (thread == NULL || entry == NULL)
        return EINVAL;
    if (xila_process.process == NULL)
        return ENOTSUP;

    size_t stack_size = attributes == NULL ? 0 : attributes->stack_size;
    void *allocation = NULL;
    XilaStatus status = xila_heap_allocate(xila_process.startup->heap,
                                          sizeof(struct thread_start), &allocation);
    if (status != XILA_STATUS_OK)
        return EAGAIN;
    *(struct thread_start *)allocation = (struct thread_start){entry, argument};
    status = xila_thread_spawn(xila_process.process, stack_size, start_thread, allocation, thread);
    if (status != XILA_STATUS_OK) {
        xila_heap_release(allocation);
        return status == XILA_STATUS_OUT_OF_MEMORY ? EAGAIN : xila_errno_from_status(status);
    }
    /* Spawn does not suspend, so the new worker cannot finish before detach. */
    if (attributes != NULL && attributes->detach_state == PTHREAD_CREATE_DETACHED)
        return pthread_detach(*thread);
    return 0;
}

void pthread_exit(void *result) {
    /* Main is unsupported (see pthread.h): fail before anything runs on its behalf. */
    size_t thread = XILA_MAIN_THREAD;
    (void)xila_thread_current(&thread);
    if (thread == XILA_MAIN_THREAD)
        abort();
    /* Give back the once controls whose routine this thread leaves unfinished. */
    xila_abandon_once_calls();
    xila_run_key_destructors();
    (void)xila_thread_exit(result);
    abort();
}

int pthread_join(pthread_t thread, void **result) {
    XilaStatus status = xila_thread_join(thread, result);
    return status == XILA_STATUS_NO_CHILD ? EINVAL : xila_errno_from_status(status);
}

int pthread_detach(pthread_t thread) {
    return xila_errno_from_status(xila_thread_detach(thread));
}

pthread_t pthread_self(void) {
    pthread_t thread = 1;
    (void)xila_thread_current(&thread);
    return thread;
}

int pthread_equal(pthread_t left, pthread_t right) { return left == right; }

int sched_yield(void) {
    XilaStatus status = xila_thread_yield();
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}
