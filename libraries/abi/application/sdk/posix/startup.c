/* Process startup and exit.
 *
 * crt0's _xila_start installs the API table and calls xila_main, which lives here so
 * that an image links the POSIX layer only when it uses it. exit() and _exit() leave
 * from any depth by jumping back to this frame: a Xila application ends by returning
 * from its entry point. The one exception is a detected stack smash, which must not
 * trust this frame or the stack above it (stack_protector.c).
 *
 * exit() runs the registered handlers and flushes every open stream before it calls
 * _exit(); _exit() flushes nothing, as POSIX requires, and only the descriptors the
 * program left open are closed on the way out. */
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "internal.h"

extern int main(int argc, char **argv);

/* The atexit() table (onexit.c); weak so that an image which never registers a handler
   does not link the table. */
extern void xila_run_exit_handlers(int status) __attribute__((weak));

/* The stack protector guard (stack_protector.c); weak for the same reason. */
extern void xila_stack_guard_seed(void) __attribute__((weak));

static jmp_buf *exit_target;
static int exit_status;

/* One argv block on the process heap: pointer array, then the NUL-terminated copies. */
static char **build_arguments(const XilaStartup *startup, int *argc) {
    size_t count = startup->argument_count;
    size_t bytes = (count + 2) * sizeof(char *) + 1; /* argv[0] is "" */
    for (size_t index = 0; index < count; ++index) {
        if (startup->arguments[index].length > SIZE_MAX / 2 - bytes - 1)
            return NULL;
        bytes += startup->arguments[index].length + 1;
    }
    char **argv = xila_heap_alloc(bytes);
    if (argv == NULL)
        return NULL;
    char *text = (char *)(argv + count + 2);
    argv[0] = text;
    *text++ = '\0';
    for (size_t index = 0; index < count; ++index) {
        argv[index + 1] = text;
        memcpy(text, startup->arguments[index].data, startup->arguments[index].length);
        text += startup->arguments[index].length;
        *text++ = '\0';
    }
    argv[count + 1] = NULL;
    *argc = (int)count + 1;
    return argv;
}

/* The host does not pass the program name yet: argv[0] is the empty string. */
__attribute__((no_stack_protector)) int xila_posix_initialize(const XilaStartup *startup, int *argc,
                                                             char ***argv) {
    if (xila_fd_initialize(startup) != 0)
        return -1;
    /* Before any function built with the stack protector runs. */
    if (xila_stack_guard_seed != NULL)
        xila_stack_guard_seed();
    *argv = build_arguments(startup, argc);
    return *argv != NULL ? 0 : -1;
}

/* What is left after the program stopped, whichever way it did: the descriptors it left
   open and the process state. Stream buffers are not touched here (see exit()). */
__attribute__((no_stack_protector)) void xila_posix_shutdown(void) {
    xila_fd_close_all();
    xila_heap_release(xila_process.cwd);
    xila_process.cwd = NULL;
}

int32_t xila_main(const XilaStartup *startup) {
    int argc = 0;
    char **argv = NULL;
    if (xila_posix_initialize(startup, &argc, &argv) != 0)
        return -1;
    jmp_buf target;
    exit_target = &target;
    if (setjmp(target) == 0) {
        int status = main(argc, argv);
        exit(status);
    }
    exit_target = NULL;
    int status = exit_status;
    xila_posix_shutdown();
    xila_process.startup = NULL;
    xila_process.process = NULL;
    return status;
}

/* picolibc tracks every FILE the application opened (stdio-exit-flush; see
   exit_flush.c), so fflush(NULL) reaches the streams that were never closed, as POSIX
   exit() requires, and stdout and stderr. It does not forget them: a program that
   called fflush(NULL) itself still gets its later writes flushed here. */
void exit(int status) {
    if (xila_run_exit_handlers != NULL)
        xila_run_exit_handlers(status);
    (void)fflush(NULL);
    _exit(status);
}

void _exit(int status) {
    exit_status = status;
    if (exit_target != NULL)
        longjmp(*exit_target, 1);
    /* Outside an application there is nowhere to go back to: stop instead of spinning. */
    __builtin_trap();
}
