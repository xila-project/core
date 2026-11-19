/* Process startup and exit.
 *
 * crt0's _xila_start installs the API table and calls xila_main, which lives here so
 * that an image links the POSIX layer only when it uses it. exit() and _exit() leave
 * from any depth by jumping back to this frame: a Xila application ends by returning
 * from its entry point, never by killing the coroutine. */
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "internal.h"

extern int main(int argc, char **argv);

/* Weak, so that an image which never touches a standard stream does not link its FILE
   and buffer just to flush it. */
extern FILE *const stdout __attribute__((weak));
extern FILE *const stderr __attribute__((weak));

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
int xila_posix_initialize(const XilaStartup *startup, int *argc, char ***argv) {
    if (xila_fd_initialize(startup) != 0)
        return -1;
    *argv = build_arguments(startup, argc);
    return *argv != NULL ? 0 : -1;
}

void xila_posix_shutdown(void) {
    /* picolibc tracks every FILE the application opened (stdio-exit-flush), so a stream
       that was never closed still reaches its file, as POSIX exit() requires. */
    (void)fflush(NULL);
    if (&stdout != NULL)
        (void)fflush(stdout);
    if (&stderr != NULL)
        (void)fflush(stderr);
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
    return status;
}

void _exit(int status) {
    exit_status = status;
    if (exit_target != NULL)
        longjmp(*exit_target, 1);
    for (;;) {
    } /* _exit outside an application cannot return: there is nowhere to go. */
}
