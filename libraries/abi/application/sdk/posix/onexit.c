/* Exit handlers for atexit(), on_exit() and __cxa_atexit().
 *
 * picolibc keeps these in a table of its own and runs it from a .fini_array entry: it
 * assumes the C runtime walks the fini array at exit. The loader runs no destructors, so
 * with picolibc's table a handler registered by an application would never be called.
 * This file provides picolibc's _on_exit (the function atexit.c, onexit.c and
 * cxa-atexit.c call) and the function exit() uses to run the table, in one archive
 * member: a program that registers no handler links neither the table nor this code,
 * and exit() finds xila_run_exit_handlers undefined (it references it weakly).
 *
 * Handlers run in the reverse order of registration, each one removed before it is
 * called, so a handler may register another or call exit() itself. */
#include <limits.h>
#include <stddef.h>

/* picolibc's local-onexit.h, which is not installed: the numbers are part of the
   contract between its three callers and _on_exit. */
enum { KIND_EMPTY, KIND_ON_EXIT, KIND_ATEXIT, KIND_CXA_ATEXIT };

union on_exit_func {
    void (*on_exit)(int, void *);
    void (*atexit)(void);
    void (*cxa_atexit)(void *);
};

struct on_exit {
    union on_exit_func func;
    void *argument;
    int kind;
};

static struct on_exit handlers[ATEXIT_MAX];

int _on_exit(int kind, union on_exit_func func, void *argument) {
    for (size_t index = 0; index < ATEXIT_MAX; ++index) {
        if (handlers[index].kind == KIND_EMPTY) {
            handlers[index].func = func;
            handlers[index].argument = argument;
            handlers[index].kind = kind;
            return 0;
        }
    }
    return -1;
}

void xila_run_exit_handlers(int status) {
    for (;;) {
        size_t index = ATEXIT_MAX;
        while (index > 0 && handlers[index - 1].kind == KIND_EMPTY)
            --index;
        if (index == 0)
            return;
        struct on_exit handler = handlers[index - 1];
        handlers[index - 1].kind = KIND_EMPTY;
        switch (handler.kind) {
        case KIND_ON_EXIT:
            handler.func.on_exit(status, handler.argument);
            break;
        case KIND_ATEXIT:
            handler.func.atexit();
            break;
        case KIND_CXA_ATEXIT:
            handler.func.cxa_atexit(handler.argument);
            break;
        }
    }
}
