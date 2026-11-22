#ifndef _XILA_PTHREAD_H
#define _XILA_PTHREAD_H

#include <stddef.h>
#include <sys/types.h>
#include <time.h>
#include <xila/application.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef size_t pthread_t;
typedef size_t pthread_key_t;
typedef XilaMutex pthread_mutex_t;

typedef struct {
    pthread_mutex_t mutex;
    bool done;
} pthread_once_t;

typedef struct {
    size_t stack_size;
    int detach_state;
} pthread_attr_t;

typedef struct {
    int type;
} pthread_mutexattr_t;

typedef struct {
    uint32_t clock;
} pthread_condattr_t;

typedef struct {
    XilaCondition condition;
    uint32_t clock;
} pthread_cond_t;

/* A process-private read-write lock: an ABI mutex and two condition variables around
   its state. Keep its address fixed from first use through destruction. */
typedef struct {
    pthread_mutex_t mutex;
    XilaCondition readers_queue;
    XilaCondition writers_queue;
    uint32_t readers;
    pthread_t writer;
} pthread_rwlock_t;

typedef struct {
    int shared;
} pthread_rwlockattr_t;

#define PTHREAD_CREATE_JOINABLE 0
#define PTHREAD_CREATE_DETACHED 1
#define PTHREAD_MUTEX_NORMAL 0
#define PTHREAD_MUTEX_DEFAULT PTHREAD_MUTEX_NORMAL
#define PTHREAD_MUTEX_RECURSIVE 1
#define PTHREAD_MUTEX_ERRORCHECK 2
#define PTHREAD_MUTEX_INITIALIZER {0, 0, false, NULL}
#define PTHREAD_COND_INITIALIZER {{NULL}, XILA_CLOCK_MONOTONIC}
#define PTHREAD_ONCE_INIT {PTHREAD_MUTEX_INITIALIZER, false}
#define PTHREAD_RWLOCK_INITIALIZER {PTHREAD_MUTEX_INITIALIZER, {NULL}, {NULL}, 0, 0}
#define PTHREAD_PROCESS_PRIVATE 0
#define PTHREAD_PROCESS_SHARED 1
#define PTHREAD_DESTRUCTOR_ITERATIONS 4
#ifndef PTHREAD_STACK_MIN
#define PTHREAD_STACK_MIN XILA_THREAD_STACK_MIN
#endif

int pthread_attr_init(pthread_attr_t *attributes);
int pthread_attr_destroy(pthread_attr_t *attributes);
int pthread_attr_setstacksize(pthread_attr_t *attributes, size_t size);
int pthread_attr_getstacksize(const pthread_attr_t *attributes, size_t *size);
int pthread_attr_setdetachstate(pthread_attr_t *attributes, int state);
int pthread_attr_getdetachstate(const pthread_attr_t *attributes, int *state);
int pthread_create(pthread_t *thread, const pthread_attr_t *attributes, void *(*entry)(void *),
                   void *argument);
/* Supported from worker entries; on main it aborts at once. Use return or exit instead. */
void pthread_exit(void *result) __attribute__((__noreturn__));
/* A routine that leaves through pthread_exit leaves the control as if never called. */
int pthread_once(pthread_once_t *once, void (*initialize)(void));
int pthread_join(pthread_t thread, void **result);
int pthread_detach(pthread_t thread);
pthread_t pthread_self(void);
int pthread_equal(pthread_t left, pthread_t right);

int pthread_key_create(pthread_key_t *key, void (*destructor)(void *));
int pthread_key_delete(pthread_key_t key);
void *pthread_getspecific(pthread_key_t key);
int pthread_setspecific(pthread_key_t key, const void *value);

int pthread_mutexattr_init(pthread_mutexattr_t *attributes);
int pthread_mutexattr_destroy(pthread_mutexattr_t *attributes);
int pthread_mutexattr_settype(pthread_mutexattr_t *attributes, int type);
int pthread_mutexattr_gettype(const pthread_mutexattr_t *attributes, int *type);
int pthread_mutex_init(pthread_mutex_t *mutex, const pthread_mutexattr_t *attributes);
int pthread_mutex_destroy(pthread_mutex_t *mutex);
int pthread_mutex_lock(pthread_mutex_t *mutex);
int pthread_mutex_trylock(pthread_mutex_t *mutex);
int pthread_mutex_unlock(pthread_mutex_t *mutex);

int pthread_condattr_init(pthread_condattr_t *attributes);
int pthread_condattr_destroy(pthread_condattr_t *attributes);
int pthread_condattr_setclock(pthread_condattr_t *attributes, clockid_t clock);
int pthread_condattr_getclock(const pthread_condattr_t *attributes, clockid_t *clock);
int pthread_cond_init(pthread_cond_t *condition, const pthread_condattr_t *attributes);
int pthread_cond_destroy(pthread_cond_t *condition);
int pthread_cond_wait(pthread_cond_t *condition, pthread_mutex_t *mutex);
int pthread_cond_timedwait(pthread_cond_t *condition, pthread_mutex_t *mutex,
                           const struct timespec *deadline);
int pthread_cond_signal(pthread_cond_t *condition);
int pthread_cond_broadcast(pthread_cond_t *condition);

int pthread_rwlockattr_init(pthread_rwlockattr_t *attributes);
int pthread_rwlockattr_destroy(pthread_rwlockattr_t *attributes);
int pthread_rwlockattr_getpshared(const pthread_rwlockattr_t *attributes, int *shared);
/* PTHREAD_PROCESS_SHARED fails with ENOTSUP: processes share no memory. */
int pthread_rwlockattr_setpshared(pthread_rwlockattr_t *attributes, int shared);
int pthread_rwlock_init(pthread_rwlock_t *lock, const pthread_rwlockattr_t *attributes);
int pthread_rwlock_destroy(pthread_rwlock_t *lock);
/* Readers are preferred: a reader enters whenever no writer holds the lock. */
int pthread_rwlock_rdlock(pthread_rwlock_t *lock);
int pthread_rwlock_tryrdlock(pthread_rwlock_t *lock);
/* The deadline is absolute, on CLOCK_REALTIME. */
int pthread_rwlock_timedrdlock(pthread_rwlock_t *lock, const struct timespec *deadline);
/* The deadline is absolute, on CLOCK_MONOTONIC or CLOCK_REALTIME. */
int pthread_rwlock_clockrdlock(pthread_rwlock_t *lock, clockid_t clock,
                               const struct timespec *deadline);
int pthread_rwlock_wrlock(pthread_rwlock_t *lock);
int pthread_rwlock_trywrlock(pthread_rwlock_t *lock);
int pthread_rwlock_timedwrlock(pthread_rwlock_t *lock, const struct timespec *deadline);
int pthread_rwlock_clockwrlock(pthread_rwlock_t *lock, clockid_t clock,
                               const struct timespec *deadline);
int pthread_rwlock_unlock(pthread_rwlock_t *lock);

#ifdef __cplusplus
}
#endif

#endif
