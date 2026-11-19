/* picolibc is built with -Derrno-function=__xila_errno_location and without TLS.
 *
 * errno belongs to the running coroutine. Applications run on a single coroutine today,
 * so the state is process-wide. Once the ABI exposes coroutine-local storage (core#718)
 * only xila_libc_state() changes: it will return the state kept in a stackful TLS slot.
 * strtok, rand and the other libc statics stay process-wide until then, because
 * picolibc keeps them in ordinary statics when thread-local storage is off. */
#include "internal.h"

struct xila_libc_state {
    int error;
};

static struct xila_libc_state state;

static struct xila_libc_state *xila_libc_state(void) { return &state; }

int *__xila_errno_location(void) { return &xila_libc_state()->error; }
