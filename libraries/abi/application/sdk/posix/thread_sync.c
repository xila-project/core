#include <pthread.h>

#include "internal.h"

int pthread_mutexattr_init(pthread_mutexattr_t *attributes) {
    attributes->type = PTHREAD_MUTEX_DEFAULT;
    return 0;
}

int pthread_mutexattr_destroy(pthread_mutexattr_t *attributes) {
    (void)attributes;
    return 0;
}

int pthread_mutexattr_settype(pthread_mutexattr_t *attributes, int type) {
    if (type != PTHREAD_MUTEX_NORMAL && type != PTHREAD_MUTEX_RECURSIVE &&
        type != PTHREAD_MUTEX_ERRORCHECK)
        return EINVAL;
    attributes->type = type;
    return 0;
}

int pthread_mutexattr_gettype(const pthread_mutexattr_t *attributes, int *type) {
    *type = attributes->type;
    return 0;
}

int pthread_mutex_init(pthread_mutex_t *mutex, const pthread_mutexattr_t *attributes) {
    uint32_t flags = attributes != NULL && attributes->type == PTHREAD_MUTEX_RECURSIVE
                         ? XILA_MUTEX_RECURSIVE : 0;
    return xila_errno_from_status(xila_mutex_init(mutex, flags));
}

int pthread_mutex_destroy(pthread_mutex_t *mutex) {
    return xila_errno_from_status(xila_mutex_destroy(mutex));
}

int pthread_mutex_lock(pthread_mutex_t *mutex) {
    return xila_errno_from_status(xila_mutex_lock(mutex, 0));
}

int pthread_mutex_trylock(pthread_mutex_t *mutex) {
    XilaStatus status = xila_mutex_lock(mutex, XILA_MUTEX_TRY);
    return status == XILA_STATUS_WOULD_BLOCK ? EBUSY : xila_errno_from_status(status);
}

int pthread_mutex_unlock(pthread_mutex_t *mutex) {
    return xila_errno_from_status(xila_mutex_unlock(mutex));
}

int pthread_condattr_init(pthread_condattr_t *attributes) {
    attributes->clock = XILA_CLOCK_MONOTONIC;
    return 0;
}

int pthread_condattr_destroy(pthread_condattr_t *attributes) {
    (void)attributes;
    return 0;
}

int pthread_condattr_setclock(pthread_condattr_t *attributes, clockid_t clock) {
    if (clock != CLOCK_MONOTONIC && clock != CLOCK_REALTIME)
        return EINVAL;
    attributes->clock = clock == CLOCK_MONOTONIC ? XILA_CLOCK_MONOTONIC : XILA_CLOCK_REALTIME;
    return 0;
}

int pthread_condattr_getclock(const pthread_condattr_t *attributes, clockid_t *clock) {
    *clock = attributes->clock == XILA_CLOCK_MONOTONIC ? CLOCK_MONOTONIC : CLOCK_REALTIME;
    return 0;
}

int pthread_cond_init(pthread_cond_t *condition, const pthread_condattr_t *attributes) {
    condition->clock = attributes == NULL ? XILA_CLOCK_MONOTONIC : attributes->clock;
    return xila_errno_from_status(xila_condition_init(&condition->condition));
}

int pthread_cond_destroy(pthread_cond_t *condition) {
    return xila_errno_from_status(xila_condition_destroy(&condition->condition));
}

int pthread_cond_wait(pthread_cond_t *condition, pthread_mutex_t *mutex) {
    return xila_errno_from_status(xila_condition_wait(&condition->condition, mutex,
                                                     condition->clock, NULL));
}

int xila_deadline_nanoseconds(const struct timespec *deadline, int64_t *nanoseconds) {
    const int64_t nanoseconds_per_second = 1000000000;
    if (deadline == NULL || deadline->tv_nsec < 0 || deadline->tv_nsec >= nanoseconds_per_second)
        return EINVAL;
    if (__builtin_mul_overflow(deadline->tv_sec, nanoseconds_per_second, nanoseconds) ||
        __builtin_add_overflow(*nanoseconds, deadline->tv_nsec, nanoseconds))
        return EOVERFLOW;
    return 0;
}

int pthread_cond_timedwait(pthread_cond_t *condition, pthread_mutex_t *mutex,
                           const struct timespec *deadline) {
    int64_t nanoseconds;
    int error = xila_deadline_nanoseconds(deadline, &nanoseconds);
    if (error != 0)
        return error;
    return xila_errno_from_status(xila_condition_wait(&condition->condition, mutex,
                                                     condition->clock, &nanoseconds));
}

int pthread_cond_signal(pthread_cond_t *condition) {
    return xila_errno_from_status(xila_condition_notify(&condition->condition, false));
}

int pthread_cond_broadcast(pthread_cond_t *condition) {
    return xila_errno_from_status(xila_condition_notify(&condition->condition, true));
}

int pthread_once(pthread_once_t *once, void (*initialize)(void)) {
    XilaStatus status = xila_mutex_lock(&once->mutex, 0);
    /* An entry run without a process driver has no concurrent callers. */
    if (status != XILA_STATUS_OK && status != XILA_STATUS_UNSUPPORTED &&
        status != XILA_STATUS_UNAVAILABLE)
        return xila_errno_from_status(status);
    if (!once->done) {
        initialize();
        once->done = true;
    }
    if (status == XILA_STATUS_OK)
        return pthread_mutex_unlock(&once->mutex);
    return 0;
}
