#include <string.h>

#include "internal.h"

char *strtok(char *restrict string, const char *restrict delimiters) {
    static char *fallback;
    XilaThreadLibc *state = xila_libc_state();
    return strtok_r(string, delimiters, state != NULL ? &state->strtok_next : &fallback);
}
