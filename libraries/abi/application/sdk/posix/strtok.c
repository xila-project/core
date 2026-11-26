#include <string.h>

#include "internal.h"

/* File scope so xila_strtok_reset can clear it: a relaunch of the same linked program
   must not continue a tokenization that pointed into the previous run's freed heap. */
static char *fallback;

char *strtok(char *restrict string, const char *restrict delimiters) {
    XilaThreadLibc *state = xila_libc_state();
    return strtok_r(string, delimiters, state != NULL ? &state->strtok_next : &fallback);
}

/* startup.c calls this hook through a weak no-op default when the same linked program relaunches. */
__attribute__((visibility("hidden"))) void xila_strtok_reset(void) {
    XilaThreadLibc *state = xila_libc_state();
    if (state != NULL)
        state->strtok_next = NULL;
    fallback = NULL;
}
