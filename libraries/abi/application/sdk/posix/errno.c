/* The loader has no ELF TLS. Use the state in the running coroutine, with a
 * fallback for entries run without a process driver. */
#include <stdlib.h>
#include <string.h>

#include "internal.h"

static XilaThreadLibc fallback = {.random_state = 1};

static XilaThreadLibc *libc_state(void) {
    XilaThreadLibc *state = &fallback;
    (void)xila_thread_libc(&state);
    return state;
}

int *__xila_errno_location(void) { return &libc_state()->error; }

char *strtok(char *restrict string, const char *restrict delimiters) {
    return strtok_r(string, delimiters, &libc_state()->strtok_next);
}

/* Preserve the pinned picolibc sequence, including the shared rand/random
   seed. rand_r already keeps its state in caller-provided storage. */
long random(void) {
    XilaThreadLibc *state = libc_state();
    state->random_state = state->random_state * UINT64_C(6364136223846793005) + 1;
    return (long)((state->random_state >> 32) & UINT32_C(0x7fffffff));
}

void srandom(unsigned int seed) { libc_state()->random_state = seed; }

int rand(void) { return (int)(random() & RAND_MAX); }
void srand(unsigned int seed) { srandom(seed); }
