/* Process startup and exit.
 *
 * crt0's _xila_start installs the API table and calls xila_main, which lives here so
 * that an image links the POSIX layer only when it uses it. Only the thread
 * calling exit continues during terminal cleanup. Main returns through its
 * saved frame; a worker returns the status through the host process API. */
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

/* Only a strong reference to environ extracts the member with the builder.
   The hidden default lets the linker resolve the call without a GOT slot. */
__attribute__((weak, visibility("hidden"), noinline)) int xila_environ_initialize(void) {
    return 0;
}

static jmp_buf *exit_target;
static int exit_status;

/* One argv block on the process heap: pointer array, then the NUL-terminated copies. */
static char **build_arguments(const XilaStartup *startup, int *argc) {
    size_t count = startup->argument_count;
    XilaString name = startup->program_name;
    size_t bytes = (count + 2) * sizeof(char *) + name.length + 1;
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
    if (name.length != 0)
        memcpy(text, name.data, name.length);
    text += name.length;
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

__attribute__((no_stack_protector)) int xila_posix_initialize(const XilaStartup *startup, int *argc,
                                                             char ***argv) {
    if (xila_fd_initialize(startup) != 0)
        return -1;
    /* Before any function built with the stack protector runs. */
    if (xila_stack_guard_seed != NULL)
        xila_stack_guard_seed();
    *argv = build_arguments(startup, argc);
    if (*argv == NULL)
        return -1;
    return xila_environ_initialize();
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

void exit(int status) {
    bool live = false;
    (void)xila_thread_has_workers(&live);
    if (!live && xila_run_exit_handlers != NULL) {
        xila_run_exit_handlers(status);
        (void)xila_thread_has_workers(&live);
    }
    if (live) {
        (void)xila_thread_stop_others(xila_process.process);
        xila_stdio_exit_with_stopped_threads();
    } else {
        (void)fflush(NULL);
    }
    _exit(status);
}

void _exit(int status) {
    (void)xila_thread_stop_others(xila_process.process);
    size_t thread = XILA_MAIN_THREAD;
    (void)xila_thread_current(&thread);
    if (thread != XILA_MAIN_THREAD) {
        /* The jump target belongs to main's stack. Finish cleanup here. */
        xila_posix_shutdown();
        (void)xila_process_exit(xila_process.process, status);
        __builtin_trap();
    }
    exit_status = status;
    if (exit_target != NULL)
        longjmp(*exit_target, 1);
    /* Outside an application there is nowhere to go back to. */
    __builtin_trap();
}
