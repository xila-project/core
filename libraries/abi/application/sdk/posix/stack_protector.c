/* Stack protector runtime: the guard value and the failure handler that objects compiled
 * with -fstack-protector, -fstack-protector-strong or -fstack-protector-all reference.
 * The handler ends the process through xila_process_exit, never through the longjmp that
 * exit() uses (see __stack_chk_fail).
 *
 * picolibc's own member seeds the guard from a constructor, which the loader rejects, so
 * the SDK removes it from libc.a (DROPPED_MEMBERS in tools/sdk/build.py). This file takes
 * its place and is linked only by an image that uses the protector. xila_posix_initialize
 * seeds the guard (weakly, so an image without it does not link this file) before any
 * function that is protected runs: crt0, xila_main and the code that seeds the guard are
 * built without the protector, as is this file. */
#include <stdint.h>
#include <string.h>
#include <unistd.h>

#include "internal.h"

#define NO_PROTECTOR __attribute__((no_stack_protector))

/* Used if the host has no entropy: a fixed value whose lowest byte is NUL, so that string
   functions overflowing into the canary stop at it. Better than nothing, not secret. */
uintptr_t __stack_chk_guard = (uintptr_t)0x595e9fbd94fda700ULL;

NO_PROTECTOR void xila_stack_guard_seed(void) {
    uintptr_t value = 0;
    if (getentropy(&value, sizeof value) != 0)
        return;
    __stack_chk_guard = value & ~(uintptr_t)0xff;
}

/* Leaves without returning through, or jumping back into, any frame of the program: the
   frames of a smashed stack hold attacker-controlled return addresses and a jmp_buf that
   _exit()'s longjmp would trust lives in xila_main's frame on that very stack.
   Like abort(), nothing is flushed (stream buffers may be corrupt) and no handler runs.
   The process ends through the process capability, whose pointer is kept in static
   storage. The capability itself, the startup block and the host's process resources
   still live on the coroutine stack (#766), so a large enough overflow can still crash
   the host before this handler finishes. */
NO_PROTECTOR __attribute__((noreturn)) void __stack_chk_fail(void) {
    static const char message[] = "*** stack smashing detected ***\n";
    /* Straight to the host's standard error, through the descriptor table on the heap. */
    (void)write(2, message, sizeof message - 1);
    /* The pointer is copied to static storage at startup so it is not read from the
       startup block on the coroutine stack. What it points to is still on that stack
       (#766), as are the startup block read by xila_posix_shutdown() and the stderr
       stream behind write(2). */
    const XilaProcess *process = xila_process.process;
    if (process != NULL) {
        xila_posix_shutdown();
        /* Status 128 + SIGABRT, what a POSIX shell reports for abort(). Does not return
           when it can end the process: the coroutine is abandoned, not unwound. */
        (void)xila_process_exit(process, 134);
    }
    /* No process capability (the host does not offer exit), or xila_process_exit()
       failed (a second shutdown does nothing): the status travels through the jump in
       _exit(), which is as far as the layer can trust the stack. */
    _exit(134);
}
