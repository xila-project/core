/* Read-write locks on the ABI's mutex and condition variables.
 *
 * Readers are preferred, as in glibc and musl: a reader enters whenever no writer holds
 * the lock, even if writers wait, so a thread may take the read lock again while it
 * holds it. A writer waits until no reader and no writer holds the lock. Waiters queue
 * on the conditions, which wake them in arrival order, and suspend so peers can run. */
#include <pthread.h>

#include "internal.h"

int pthread_rwlockattr_init(pthread_rwlockattr_t *attributes) {
    attributes->shared = PTHREAD_PROCESS_PRIVATE;
    return 0;
}

int pthread_rwlockattr_destroy(pthread_rwlockattr_t *attributes) {
    (void)attributes;
    return 0;
}

int pthread_rwlockattr_getpshared(const pthread_rwlockattr_t *attributes, int *shared) {
    *shared = attributes->shared;
    return 0;
}

/* Processes share no memory, so only process-private locks exist. */
int pthread_rwlockattr_setpshared(pthread_rwlockattr_t *attributes, int shared) {
    if (shared == PTHREAD_PROCESS_SHARED)
        return ENOTSUP;
    if (shared != PTHREAD_PROCESS_PRIVATE)
        return EINVAL;
    attributes->shared = shared;
    return 0;
}

int pthread_rwlock_init(pthread_rwlock_t *lock, const pthread_rwlockattr_t *attributes) {
    if (attributes != NULL && attributes->shared != PTHREAD_PROCESS_PRIVATE)
        return ENOTSUP;
    XilaStatus status = xila_mutex_init(&lock->mutex, 0);
    if (status == XILA_STATUS_OK)
        status = xila_condition_init(&lock->readers_queue);
    if (status == XILA_STATUS_OK)
        status = xila_condition_init(&lock->writers_queue);
    if (status != XILA_STATUS_OK)
        return xila_errno_from_status(status);
    lock->readers = 0;
    lock->writer = 0;
    return 0;
}

int pthread_rwlock_destroy(pthread_rwlock_t *lock) {
    if (lock->readers != 0 || lock->writer != 0)
        return EBUSY;
    XilaStatus status = xila_condition_destroy(&lock->readers_queue);
    if (status == XILA_STATUS_OK)
        status = xila_condition_destroy(&lock->writers_queue);
    if (status == XILA_STATUS_OK)
        status = xila_mutex_destroy(&lock->mutex);
    return xila_errno_from_status(status);
}

enum acquire { ACQUIRE_WAIT, ACQUIRE_TRY };

/* Take the lock for reading or writing. `deadline` is null to wait without a limit. As
   POSIX specifies, a deadline is checked only when the call would block. */
static int acquire(pthread_rwlock_t *lock, bool write, enum acquire mode, uint32_t clock,
                   const struct timespec *deadline) {
    pthread_t self = pthread_self();
    XilaStatus status = xila_mutex_lock(&lock->mutex, 0);
    if (status != XILA_STATUS_OK)
        return xila_errno_from_status(status);
    int error = 0;
    bool blocked = write ? lock->writer != 0 || lock->readers != 0 : lock->writer != 0;
    int64_t nanoseconds = 0;
    if (blocked) {
        if (mode == ACQUIRE_TRY)
            error = EBUSY;
        else if (lock->writer == self)
            error = EDEADLK;
        else if (deadline != NULL)
            error = xila_deadline_nanoseconds(deadline, &nanoseconds);
    }
    XilaCondition *queue = write ? &lock->writers_queue : &lock->readers_queue;
    while (error == 0 && blocked) {
        status = xila_condition_wait(queue, &lock->mutex, clock,
                                     deadline == NULL ? NULL : &nanoseconds);
        if (status != XILA_STATUS_OK)
            error = xila_errno_from_status(status);
        blocked = write ? lock->writer != 0 || lock->readers != 0 : lock->writer != 0;
    }
    if (error == 0) {
        if (write)
            lock->writer = self;
        else if (lock->readers == UINT32_MAX)
            error = EAGAIN;
        else
            ++lock->readers;
    }
    status = xila_mutex_unlock(&lock->mutex);
    if (error == 0 && status != XILA_STATUS_OK)
        error = xila_errno_from_status(status);
    return error;
}

static int clock_of(clockid_t clock, uint32_t *native) {
    if (clock != CLOCK_MONOTONIC && clock != CLOCK_REALTIME)
        return EINVAL;
    *native = clock == CLOCK_MONOTONIC ? XILA_CLOCK_MONOTONIC : XILA_CLOCK_REALTIME;
    return 0;
}

int pthread_rwlock_rdlock(pthread_rwlock_t *lock) {
    return acquire(lock, false, ACQUIRE_WAIT, XILA_CLOCK_MONOTONIC, NULL);
}

int pthread_rwlock_tryrdlock(pthread_rwlock_t *lock) {
    return acquire(lock, false, ACQUIRE_TRY, XILA_CLOCK_MONOTONIC, NULL);
}

int pthread_rwlock_timedrdlock(pthread_rwlock_t *lock, const struct timespec *deadline) {
    if (deadline == NULL)
        return EINVAL;
    return acquire(lock, false, ACQUIRE_WAIT, XILA_CLOCK_REALTIME, deadline);
}

int pthread_rwlock_clockrdlock(pthread_rwlock_t *lock, clockid_t clock,
                               const struct timespec *deadline) {
    uint32_t native;
    if (deadline == NULL || clock_of(clock, &native) != 0)
        return EINVAL;
    return acquire(lock, false, ACQUIRE_WAIT, native, deadline);
}

int pthread_rwlock_wrlock(pthread_rwlock_t *lock) {
    return acquire(lock, true, ACQUIRE_WAIT, XILA_CLOCK_MONOTONIC, NULL);
}

int pthread_rwlock_trywrlock(pthread_rwlock_t *lock) {
    return acquire(lock, true, ACQUIRE_TRY, XILA_CLOCK_MONOTONIC, NULL);
}

int pthread_rwlock_timedwrlock(pthread_rwlock_t *lock, const struct timespec *deadline) {
    if (deadline == NULL)
        return EINVAL;
    return acquire(lock, true, ACQUIRE_WAIT, XILA_CLOCK_REALTIME, deadline);
}

int pthread_rwlock_clockwrlock(pthread_rwlock_t *lock, clockid_t clock,
                               const struct timespec *deadline) {
    uint32_t native;
    if (deadline == NULL || clock_of(clock, &native) != 0)
        return EINVAL;
    return acquire(lock, true, ACQUIRE_WAIT, native, deadline);
}

/* A writer leaving wakes every reader and one writer; whoever takes the mutex first
   wins, and the others wait again. The last reader leaving wakes one writer. */
int pthread_rwlock_unlock(pthread_rwlock_t *lock) {
    XilaStatus status = xila_mutex_lock(&lock->mutex, 0);
    if (status != XILA_STATUS_OK)
        return xila_errno_from_status(status);
    int error = 0;
    if (lock->writer == pthread_self()) {
        lock->writer = 0;
        status = xila_condition_notify(&lock->readers_queue, true);
        if (status == XILA_STATUS_OK)
            status = xila_condition_notify(&lock->writers_queue, false);
    } else if (lock->writer == 0 && lock->readers != 0) {
        if (--lock->readers == 0)
            status = xila_condition_notify(&lock->writers_queue, false);
    } else {
        error = EPERM;
    }
    if (error == 0 && status != XILA_STATUS_OK)
        error = xila_errno_from_status(status);
    status = xila_mutex_unlock(&lock->mutex);
    if (error == 0 && status != XILA_STATUS_OK)
        error = xila_errno_from_status(status);
    return error;
}
