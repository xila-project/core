#ifndef _XILA_SEMAPHORE_H
#define _XILA_SEMAPHORE_H

#include <limits.h>
#include <time.h>
#include <xila/application.h>

#ifdef __cplusplus
extern "C" {
#endif

/* An unnamed, process-private counting semaphore: an ABI mutex and condition variable
   around the count. Keep its address fixed from sem_init to sem_destroy. */
typedef struct {
    XilaMutex mutex;
    XilaCondition condition;
    unsigned int value;
} sem_t;

#define SEM_FAILED ((sem_t *)0)
#ifndef SEM_VALUE_MAX
#define SEM_VALUE_MAX INT_MAX
#endif

/* A nonzero pshared fails with ENOSYS: processes share no memory. */
int sem_init(sem_t *semaphore, int pshared, unsigned int value);
int sem_destroy(sem_t *semaphore);
int sem_post(sem_t *semaphore);
int sem_wait(sem_t *semaphore);
int sem_trywait(sem_t *semaphore);
/* The deadline is absolute, on CLOCK_REALTIME. */
int sem_timedwait(sem_t *semaphore, const struct timespec *deadline);
/* The deadline is absolute, on CLOCK_MONOTONIC or CLOCK_REALTIME. */
int sem_clockwait(sem_t *semaphore, clockid_t clock, const struct timespec *deadline);
int sem_getvalue(sem_t *semaphore, int *value);

/* Named semaphores are not provided: these fail with ENOSYS. */
sem_t *sem_open(const char *name, int flags, ...);
int sem_close(sem_t *semaphore);
int sem_unlink(const char *name);

#ifdef __cplusplus
}
#endif

#endif
