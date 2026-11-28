#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int initialized;

static void preinitialize(void) {
    puts("preinit");
    initialized = 1;
}

__attribute__((used, section(".preinit_array")))
static void (*const preinitializer)(void) = preinitialize;

__attribute__((constructor))
static void initialize(void) {
    puts("constructor");
    if (initialized == 1)
        initialized = 2;
}

__attribute__((destructor(101)))
static void finalize_first(void) {
    puts("destructor 1");
}

__attribute__((destructor(102)))
static void finalize_second(void) {
    puts("destructor 2");
}

static void on_exit_handler(void) {
    puts("atexit");
}

int main(int argc, char **argv) {
    if (initialized != 2 || atexit(on_exit_handler) != 0)
        return 1;
    puts("main");
    if (argc > 1 && strcmp(argv[1], "_exit") == 0) {
        fflush(stdout);
        _exit(23);
    }
    if (argc > 1 && strcmp(argv[1], "exit") == 0)
        exit(23);
    return 23;
}
