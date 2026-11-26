/* A linked C program fixture for tests/linked.rs. The build script compiles two copies
   of this file (one per LABEL), localizes each with llvm-objcopy so that only the entry
   symbol stays global, and archives both into one library the test binary links. Both
   copies define `helper`, `malloc` and `static int state` on purpose: without the
   localization they would collide at link time, and two concurrent runs would share the
   state.

   The fixture is freestanding C11 on purpose: it includes no host libc header, so a build
   host without them still compiles it (the only headers are the ABI's own). */
#include "application.h"

#ifndef LABEL
#define LABEL "unknown"
#endif

/* Colliding globals: both copies of this file define them. */
int helper(int value) { return value + 1; }
static int state = 0;
void *malloc(size_t size) {
    (void)size;
    return &state;
}

/* Append a decimal integer to `text`, returning the new end. */
static char *put_int(char *text, int value) {
    char digits[12];
    int count = 0;
    unsigned magnitude = value < 0 ? 0u - (unsigned)value : (unsigned)value;
    do {
        digits[count++] = (char)('0' + magnitude % 10);
        magnitude /= 10;
    } while (magnitude != 0);
    if (value < 0)
        *text++ = '-';
    while (count > 0)
        *text++ = digits[--count];
    return text;
}

int32_t xila_linked_start(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    /* The first argument names this run; without one, the compiled-in label does. */
    const char *who = LABEL;
    size_t who_length = sizeof(LABEL) - 1;
    if (startup->argument_count > 0) {
        who = startup->arguments[0].data;
        who_length = startup->arguments[0].length;
    }
    /* This program's own `static` persists across launches of the same localized copy (the
       SDK resets its own state, not the application's), so start every run from zero:
       this is what makes sequential runs of one program testable. */
    state = 0;
    char line[96];
    for (int step = 0; step < 3; ++step) {
        state = helper(state);
        /* Suspend, so the other program runs between the steps. */
        (void)api->time_sleep(5000000);
        char *text = line;
        for (size_t index = 0; index < who_length && text < line + sizeof line - 21; ++index)
            *text++ = who[index];
        *text++ = ' ';
        *text++ = 's';
        *text++ = 't';
        *text++ = 'a';
        *text++ = 't';
        *text++ = 'e';
        *text++ = '=';
        text = put_int(text, state);
        *text++ = '\n';
        size_t count = 0;
        (void)api->file_write(startup->standard_output, line, (size_t)(text - line), &count);
    }
    /* A shared `state` would have counted past 3 while the other program ran. */
    return state == 3 ? 0 : 1;
}
