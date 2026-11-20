/* Stack protector runtime: the guard value and the failure handler that objects compiled
 * with -fstack-protector, -fstack-protector-strong or -fstack-protector-all reference.
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

NO_PROTECTOR __attribute__((noreturn)) void __stack_chk_fail(void) {
    static const char message[] = "*** stack smashing detected ***\n";
    /* The frame that detected the corruption is trusted no further than this call: write
       straight to the host's standard error and leave with the status SIGABRT would give
       a POSIX shell (128 + 6). */
    (void)write(2, message, sizeof message - 1);
    _exit(134);
}
