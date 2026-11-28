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

/* The atexit() table (onexit.c). Weak default below: a program that registers no handler
   links no table, and exit() calls the no-op. */
extern void xila_run_exit_handlers(int status) __attribute__((weak));

/* The stack protector guard (stack_protector.c); weak default for the same reason. */
extern void xila_stack_guard_seed(void) __attribute__((weak));

/* Only a strong reference to environ extracts the member with the builder.
   The hidden default lets the linker resolve the call without a GOT slot. */
__attribute__((weak, visibility("hidden"), noinline)) int xila_environ_initialize(void) {
    return 0;
}

static jmp_buf *exit_target;
static int exit_status;
static bool finalization_started;

static void run_lifecycle(bool finalize) {
    const XilaStartup *startup = xila_process.startup;
    if (startup != NULL && startup->size >= sizeof(*startup) && startup->lifecycle != NULL)
        startup->lifecycle->run(startup->lifecycle, finalize);
}

/* Per-run reset hooks of the POSIX modules. Every module that keeps static state holding
   heap pointers, locks, buffers or per-run bookkeeping defines its hook strongly (hidden);
   the weak no-op defaults here keep the symbols defined when that module is not linked, so
   the calls below are ordinary calls to a defined symbol and an unlinked module costs no
   code beyond the no-op. A previous run of the same linked program may have ended by
   cancellation, which runs no cleanup: the host freed that run's process heap with the
   abandoned coroutine, so every static that can still point into it must be reset before
   this run touches it. */
__attribute__((weak, visibility("hidden"), noinline)) void xila_stdin_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_stdout_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_stderr_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_stdio_list_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_environ_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_env_cache_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_onexit_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_process_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_thread_sync_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_thread_keys_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_libc_lock_reset(void) {}
__attribute__((weak, visibility("hidden"), noinline)) void xila_strtok_reset(void) {}

/* Weak defaults for the two optional members above: no handler table, no stack guard. */
__attribute__((weak, visibility("hidden"), noinline)) void xila_run_exit_handlers(int status) {
    (void)status;
}
__attribute__((weak, visibility("hidden"), noinline)) void xila_stack_guard_seed(void) {}

/* Weak defaults for picolibc's optional weak function hooks, keeping their exact
   fallback behaviour (strerror uses its built-in table, arc4random sees no fork and no
   abort hook) while avoiding weak undefined PLT/PC-relative call relocations that a PIE
   host link rejects when resolved to absolute 0. Their headers declare default
   visibility; link-archive localizes them in the output archive. */
__attribute__((weak, noinline)) char *_user_strerror(int errnum, int internal, int *errptr) {
    (void)errnum;
    (void)internal;
    (void)errptr;
    return NULL;
}
__attribute__((weak, noinline)) int arc4random_fork_detect(void) { return 0; }
__attribute__((weak, noinline)) void arc4random_abort(void) {}

/* Picolibc's getopt keeps its parser cursor and flags in these globals across linked
   launches. Weak data references go through the GOT in PIE builds (so unlinked getopt
   resolves to null and costs no code) and reset getopt on every launch when linked. */
extern int optind __attribute__((weak));
extern int opterr __attribute__((weak));
extern int optopt __attribute__((weak));
extern char *optarg __attribute__((weak));

/* Drop every trace of a previous run of the same program, whichever way it ended: a
   normal exit already cleaned up, but a cancellation left stale pointers behind. */
static void reset_previous_run(void) {
    exit_target = NULL;
    exit_status = 0;
    finalization_started = false;
    xila_stdin_reset();
    xila_stdout_reset();
    xila_stderr_reset();
    xila_stdio_list_reset();
    xila_environ_reset();
    xila_env_cache_reset();
    xila_onexit_reset();
    xila_process_reset();
    xila_thread_sync_reset();
    xila_thread_keys_reset();
    xila_libc_lock_reset();
    xila_strtok_reset();
    if (&optind != NULL)
        optind = 0;
    if (&opterr != NULL)
        opterr = 1;
    if (&optopt != NULL)
        optopt = 0;
    if (&optarg != NULL)
        optarg = NULL;
}

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
    /* A relaunch of the same linked program starts from pristine runtime state, even when
       the previous run was cancelled mid-call and cleaned up nothing. */
    reset_previous_run();
    int argc = 0;
    char **argv = NULL;
    if (xila_posix_initialize(startup, &argc, &argv) != 0)
        return -1;
    jmp_buf target;
    exit_target = &target;
    if (setjmp(target) == 0) {
        run_lifecycle(false);
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
    if (!live) {
        xila_run_exit_handlers(status);
        (void)xila_thread_has_workers(&live);
        if (!live && !finalization_started) {
            finalization_started = true;
            run_lifecycle(true);
            (void)xila_thread_has_workers(&live);
        }
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
