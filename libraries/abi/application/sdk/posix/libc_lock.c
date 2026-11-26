/* picolibc retarget locks. The global libc lock and per-FILE locks use the
 * same process-local mutexes as pthreads. FILE locks are allocated lazily. */
#include <stdio.h>
#include <sys/lock.h>

#include "internal.h"

struct __lock {
    XilaMutex mutex;
};

struct __lock __lock___libc_recursive_mutex = {.mutex = {.recursive = true}};

/* Back to the pristine unlocked state, for a relaunch of the same linked program after a
   previous run was cancelled while holding the global libc lock. startup.c calls this
   hook through a weak no-op default, so the symbol is always defined. */
__attribute__((visibility("hidden"))) void xila_libc_lock_reset(void) {
    __lock___libc_recursive_mutex = (struct __lock){.mutex = {.recursive = true}};
}

static void lock_init(_LOCK_T *lock, bool recursive) {
    *lock = xila_heap_alloc(sizeof(**lock));
    if (*lock == NULL) {
        /* The picolibc hook cannot return an allocation error. Keep the lock
           private and functional by using the recursive libc lock. */
        *lock = &__lock___libc_recursive_mutex;
    } else {
        (void)xila_mutex_init(&(*lock)->mutex, recursive ? XILA_MUTEX_RECURSIVE : 0);
    }
}

void __retarget_lock_init(_LOCK_T *lock) { lock_init(lock, false); }
void __retarget_lock_init_recursive(_LOCK_T *lock) { lock_init(lock, true); }

void __retarget_lock_close(_LOCK_T lock) {
    if (lock != &__lock___libc_recursive_mutex) {
        if (xila_mutex_destroy(&lock->mutex) != XILA_STATUS_OK)
            __builtin_trap();
        xila_heap_release(lock);
    }
}
void __retarget_lock_close_recursive(_LOCK_T lock) { __retarget_lock_close(lock); }

static bool lock_succeeded(XilaStatus status) {
    /* No other thread exists when the loader runs without a process driver. */
    return status == XILA_STATUS_OK || status == XILA_STATUS_UNSUPPORTED ||
           status == XILA_STATUS_UNAVAILABLE;
}

void __retarget_lock_acquire(_LOCK_T lock) {
    if (!lock_succeeded(xila_mutex_lock(&lock->mutex, 0)))
        __builtin_trap();
}

void __retarget_lock_acquire_recursive(_LOCK_T lock) { __retarget_lock_acquire(lock); }

void __retarget_lock_release(_LOCK_T lock) {
    if (!lock_succeeded(xila_mutex_unlock(&lock->mutex)))
        __builtin_trap();
}

void __retarget_lock_release_recursive(_LOCK_T lock) { __retarget_lock_release(lock); }

/* picolibc 1.8.12's ftrylockfile takes the global lock, even when funlockfile
   uses the FILE lock. Match funlockfile and use the ABI's nonblocking probe. */
extern void __flockfile_init(FILE *file);

int ftrylockfile(FILE *file) {
    if (file->lock == NULL)
        __flockfile_init(file);
    if (file->lock == (_LOCK_T)(uintptr_t)1) /* picolibc's __LOCK_NONE */
        return 0;
    return lock_succeeded(xila_mutex_lock(&file->lock->mutex, XILA_MUTEX_TRY)) ? 0 : 1;
}
