/* The loader has no ELF TLS. Use coroutine-local errno when available. */
#include "internal.h"

int *__xila_errno_location(void) {
    static int fallback;
    XilaThreadLibc *state = xila_libc_state();
    return state != NULL ? &state->error : &fallback;
}
