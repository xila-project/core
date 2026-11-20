/* Built with -fstack-protector-strong against the SDK, to prove that such objects link and
 * that the runtime works. The first argument selects the scenario:
 *
 *   ok     protected functions return normally; the guard was seeded from getentropy
 *   smash  overwrite the canary: the process ends with status 134 and a message
 *   large  overwrite `argv[2]` bytes (default 256) past the buffer, over main's saved
 *          return address and xila_main's frame with its jmp_buf, and on up the coroutine
 *          stack: the process still ends with status 134 and the message, without
 *          returning through those frames. The host's own state is not on that stack, so
 *          the size may be anything that stays inside the stack
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern uintptr_t __stack_chk_guard;

/* Fills a local array through a volatile pointer, `count` bytes: the compiler can neither
   drop the array nor know the bound. */
__attribute__((noinline)) static int fill(size_t count) {
    char buffer[16];
    volatile char *cursor = buffer;
    for (size_t index = 0; index < count; ++index)
        cursor[index] = 'A';
    return buffer[0];
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    if (strcmp(mode, "ok") == 0) {
        /* Within bounds; the host's entropy device yields 0xa5 bytes, and the low byte of
           the guard is NUL by construction. */
        if (fill(sizeof(char[16])) != 'A')
            return 1;
        if ((__stack_chk_guard & 0xff) != 0 || ((__stack_chk_guard >> 8) & 0xff) != 0xa5) {
            fprintf(stderr, "ssp: guard %lx\n", (unsigned long)__stack_chk_guard);
            return 2;
        }
        puts("ssp: ok");
        return 0;
    }
    if (strcmp(mode, "smash") == 0) {
        fill(16 + 64);
        puts("ssp: smash returned");
        return 3;
    }
    if (strcmp(mode, "large") == 0) {
        fill(argc > 2 ? (size_t)atoi(argv[2]) : 256);
        puts("ssp: large smash returned");
        return 3;
    }
    fprintf(stderr, "ssp: unknown scenario \"%s\"\n", mode);
    return 4;
}
