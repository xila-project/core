/* The loader has no ELF TLS. Use coroutine-local errno when available. */
#include "internal.h"

/* On ILP32 GCC toolchains such as Xtensa, `int32_t` is `long int` while `errno` is `int`;
   both are 32-bit two's-complement integers with identical size and alignment, and C code
   accesses `state->error` exclusively through `__xila_errno_location`. */
_Static_assert(sizeof(int) == sizeof(int32_t) && _Alignof(int) == _Alignof(int32_t),
               "XilaThreadLibc.error must have the layout of int");

int *__xila_errno_location(void) {
    static int fallback;
    XilaThreadLibc *state = xila_libc_state();
    return state != NULL ? (int *)&state->error : &fallback;
}
