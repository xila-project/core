#include <stdlib.h>

#include "internal.h"

static uint64_t *random_state(void) {
    static uint64_t fallback = 1;
    XilaThreadLibc *state = xila_libc_state();
    return state != NULL ? &state->random_state : &fallback;
}

/* Preserve the pinned picolibc sequence and the shared rand/random seed. */
long random(void) {
    uint64_t *state = random_state();
    *state = *state * UINT64_C(6364136223846793005) + 1;
    return (long)((*state >> 32) & UINT32_C(0x7fffffff));
}

void srandom(unsigned int seed) { *random_state() = seed; }
int rand(void) { return (int)(random() & RAND_MAX); }
void srand(unsigned int seed) { srandom(seed); }
