/* Unnamed counting semaphores on the ABI's mutex and condition variable.
 *
 * Waiters queue on the condition, which wakes them in arrival order; a post wakes one.
 * Every call returns 0, or -1 with errno set, as POSIX specifies for semaphores. */
#include <semaphore.h>

#include "internal.h"

static int fail_status(XilaStatus status) {
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

int sem_init(sem_t *semaphore, int pshared, unsigned int value) {
    if (pshared != 0)
        return xila_fail_errno(ENOSYS);
    if (value > SEM_VALUE_MAX)
        return xila_fail_errno(EINVAL);
    XilaStatus status = xila_mutex_init(&semaphore->mutex, 0);
    if (status == XILA_STATUS_OK)
        status = xila_condition_init(&semaphore->condition);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    semaphore->value = value;
    return 0;
}

int sem_destroy(sem_t *semaphore) {
    XilaStatus status = xila_condition_destroy(&semaphore->condition);
    if (status == XILA_STATUS_OK)
        status = xila_mutex_destroy(&semaphore->mutex);
    return fail_status(status);
}

int sem_post(sem_t *semaphore) {
    XilaStatus status = xila_mutex_lock(&semaphore->mutex, 0);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    int error = 0;
    if (semaphore->value >= SEM_VALUE_MAX) {
        error = EOVERFLOW;
    } else {
        ++semaphore->value;
        status = xila_condition_notify(&semaphore->condition, false);
        if (status != XILA_STATUS_OK)
            error = xila_errno_from_status(status);
    }
    status = xila_mutex_unlock(&semaphore->mutex);
    if (error == 0 && status != XILA_STATUS_OK)
        error = xila_errno_from_status(status);
    return error == 0 ? 0 : xila_fail_errno(error);
}

/* Take one unit. `deadline` is null to wait without a limit; `try` fails at once. */
static int take(sem_t *semaphore, uint32_t clock, const struct timespec *deadline, int try) {
    XilaStatus status = xila_mutex_lock(&semaphore->mutex, 0);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    int error = 0;
    int64_t nanoseconds = 0;
    /* POSIX checks the deadline only when the call would block. */
    if (semaphore->value == 0) {
        if (try)
            error = EAGAIN;
        else if (deadline != NULL)
            error = xila_deadline_nanoseconds(deadline, &nanoseconds);
    }
    while (error == 0 && semaphore->value == 0) {
        status = xila_condition_wait(&semaphore->condition, &semaphore->mutex, clock,
                                     deadline == NULL ? NULL : &nanoseconds);
        if (status != XILA_STATUS_OK)
            error = xila_errno_from_status(status);
    }
    if (error == 0)
        --semaphore->value;
    status = xila_mutex_unlock(&semaphore->mutex);
    if (error == 0 && status != XILA_STATUS_OK)
        error = xila_errno_from_status(status);
    return error == 0 ? 0 : xila_fail_errno(error);
}

int sem_wait(sem_t *semaphore) { return take(semaphore, XILA_CLOCK_MONOTONIC, NULL, 0); }

int sem_trywait(sem_t *semaphore) { return take(semaphore, XILA_CLOCK_MONOTONIC, NULL, 1); }

int sem_timedwait(sem_t *semaphore, const struct timespec *deadline) {
    if (deadline == NULL)
        return xila_fail_errno(EINVAL);
    return take(semaphore, XILA_CLOCK_REALTIME, deadline, 0);
}

int sem_clockwait(sem_t *semaphore, clockid_t clock, const struct timespec *deadline) {
    if (deadline == NULL || (clock != CLOCK_MONOTONIC && clock != CLOCK_REALTIME))
        return xila_fail_errno(EINVAL);
    return take(semaphore, clock == CLOCK_MONOTONIC ? XILA_CLOCK_MONOTONIC : XILA_CLOCK_REALTIME,
                deadline, 0);
}

int sem_getvalue(sem_t *semaphore, int *value) {
    XilaStatus status = xila_mutex_lock(&semaphore->mutex, 0);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    *value = (int)semaphore->value;
    return fail_status(xila_mutex_unlock(&semaphore->mutex));
}

sem_t *sem_open(const char *name, int flags, ...) {
    (void)name;
    (void)flags;
    errno = ENOSYS;
    return SEM_FAILED;
}

int sem_close(sem_t *semaphore) {
    (void)semaphore;
    return xila_fail_errno(ENOSYS);
}

int sem_unlink(const char *name) {
    (void)name;
    return xila_fail_errno(ENOSYS);
}
