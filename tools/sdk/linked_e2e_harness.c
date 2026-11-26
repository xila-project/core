/* Host harness for the real-SDK linked-archive end-to-end test (test_link_archive.py,
   RealSdkLinkedProgramTest): runs one localized POSIX C program
   three times in one process, with a fresh poisoned-after-use heap per run, emulating the
   firmware's ProcessOwner (which frees the process heap when a run ends or is cancelled).
   Run 2 is "cancelled" mid-pipe-read: its thread is abandoned and its heap poisoned with
   mprotect(PROT_NONE), so any stale .data pointer into it faults loudly. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include "application.h"

/* ---- the localized program's entry ---- */
extern int32_t my_app(const XilaStartup *startup);

/* ---- capabilities (the host side of the ABI) ---- */
struct XilaFile { int fd; };
struct XilaProcess { uint32_t id; };
struct XilaHeap {
    char *base;
    size_t size;
    size_t used;
};

/* ---- per-run state ---- */
static const XilaApi api;

static XilaStatus h_file_read(XilaFile *file, void *buffer, size_t size, size_t *count) {
    ssize_t n = read(file->fd, buffer, size);
    if (n < 0)
        return XILA_STATUS_INPUT_OUTPUT;
    *count = (size_t)n;
    return XILA_STATUS_OK;
}

static XilaStatus h_file_write(XilaFile *file, const void *buffer, size_t size, size_t *count) {
    ssize_t n = write(file->fd, buffer, size);
    if (n < 0)
        return XILA_STATUS_INPUT_OUTPUT;
    *count = (size_t)n;
    return XILA_STATUS_OK;
}

static XilaStatus h_file_close(XilaFile *file) {
    (void)file; /* borrowed standard streams stay open */
    return XILA_STATUS_OK;
}

static XilaStatus h_heap_allocate(XilaHeap *heap, size_t size, void **allocation) {
    size_t need = 16 + ((size + 15) & ~(size_t)15);
    if (heap->used + need > heap->size)
        return XILA_STATUS_OUT_OF_MEMORY;
    char *block = heap->base + heap->used;
    heap->used += need;
    *(size_t *)block = size;
    *allocation = block + 16;
    return XILA_STATUS_OK;
}

static XilaStatus h_heap_reallocate(XilaHeap *heap, void *pointer, size_t size,
                                    void **allocation) {
    if (pointer == NULL)
        return h_heap_allocate(heap, size, allocation);
    if (size == 0) {
        *allocation = NULL;
        return XILA_STATUS_OK;
    }
    size_t old = *(size_t *)((char *)pointer - 16);
    XilaStatus status = h_heap_allocate(heap, size, allocation);
    if (status == XILA_STATUS_OK)
        memcpy(*allocation, pointer, old < size ? old : size);
    return status;
}

static XilaStatus h_heap_free(XilaHeap *heap, void *pointer) {
    (void)heap;
    (void)pointer; /* bump allocator: the whole region is poisoned at the end of the run */
    return XILA_STATUS_OK;
}

/* ---- environment ---- */
static char env_entries[8][160];
static size_t env_count;

static void env_set_internal(const char *name, size_t name_length, const char *value,
                             size_t value_length) {
    for (size_t i = 0; i < env_count; ++i) {
        if (strlen(env_entries[i]) == name_length + value_length + 1 &&
            strncmp(env_entries[i], name, name_length) == 0 &&
            env_entries[i][name_length] == '=')
            env_count--, memmove(&env_entries[i], &env_entries[i + 1],
                                 (env_count - i) * sizeof env_entries[0]), i--;
    }
    if (env_count == 8)
        return;
    int n = snprintf(env_entries[env_count], sizeof env_entries[0], "%.*s=%.*s",
                     (int)name_length, name, (int)value_length, value);
    if (n > 0)
        env_count++;
}

static XilaStatus copy_out(const char *data, size_t length, char *buffer, size_t capacity,
                           size_t *out_length) {
    if (capacity < length) {
        *out_length = length;
        return XILA_STATUS_BUFFER_TOO_SMALL;
    }
    memcpy(buffer, data, length);
    *out_length = length;
    return XILA_STATUS_OK;
}

static XilaStatus h_env_get(const XilaProcess *process, const char *name, size_t name_length,
                            char *buffer, size_t capacity, size_t *value_length) {
    (void)process;
    for (size_t i = 0; i < env_count; ++i) {
        const char *entry = env_entries[i];
        const char *equals = strchr(entry, '=');
        if ((size_t)(equals - entry) == name_length && strncmp(entry, name, name_length) == 0)
            return copy_out(equals + 1, strlen(equals + 1), buffer, capacity, value_length);
    }
    return XILA_STATUS_NOT_FOUND;
}

static XilaStatus h_env_set(const XilaProcess *process, const char *name, size_t name_length,
                            const char *value, size_t value_length) {
    (void)process;
    env_set_internal(name, name_length, value, value_length);
    return XILA_STATUS_OK;
}

static XilaStatus h_env_remove(const XilaProcess *process, const char *name, size_t name_length) {
    (void)process;
    env_set_internal(name, name_length, "", 0);
    /* remove instead of set-empty: drop entries equal to name with empty value */
    for (size_t i = 0; i < env_count; ++i)
        if (strncmp(env_entries[i], name, name_length) == 0 && env_entries[i][name_length] == '=')
            env_count--, memmove(&env_entries[i], &env_entries[i + 1],
                                 (env_count - i) * sizeof env_entries[0]), i--;
    return XILA_STATUS_OK;
}

static XilaStatus h_env_entry(const XilaProcess *process, size_t index, char *buffer,
                              size_t capacity, size_t *entry_length) {
    (void)process;
    if (index >= env_count)
        return XILA_STATUS_NOT_FOUND;
    return copy_out(env_entries[index], strlen(env_entries[index]), buffer, capacity,
                    entry_length);
}

/* ---- threads and locks ---- */
static __thread XilaThreadLibc libc_state;
static __thread const XilaStartup *current_startup;

const XilaStartup *__xila_current_startup(void) { return current_startup; }

static XilaStatus h_thread_libc(XilaThreadLibc **location) {
    *location = &libc_state;
    return XILA_STATUS_OK;
}

static XilaStatus h_thread_current(size_t *thread) {
    *thread = 1; /* XILA_MAIN_THREAD */
    return XILA_STATUS_OK;
}

static XilaStatus h_thread_has_workers(bool *live) {
    *live = false;
    return XILA_STATUS_OK;
}

static XilaStatus h_thread_stop_others(const XilaProcess *process) {
    (void)process;
    return XILA_STATUS_OK;
}

static XilaStatus h_mutex_init(XilaMutex *mutex, uint32_t flags) {
    pthread_mutex_t *impl = malloc(sizeof *impl);
    pthread_mutexattr_t attr;
    pthread_mutexattr_init(&attr);
    pthread_mutexattr_settype(&attr, (flags & XILA_MUTEX_RECURSIVE) ? PTHREAD_MUTEX_RECURSIVE
                                                                    : PTHREAD_MUTEX_NORMAL);
    pthread_mutex_init(impl, &attr);
    pthread_mutexattr_destroy(&attr);
    mutex->recursive = (flags & XILA_MUTEX_RECURSIVE) != 0;
    mutex->owner = 0;
    mutex->depth = 0;
    mutex->waiters = impl;
    return XILA_STATUS_OK;
}

static XilaStatus h_mutex_destroy(XilaMutex *mutex) {
    if (mutex->waiters != NULL) {
        pthread_mutex_t *impl = mutex->waiters;
        pthread_mutex_destroy(impl);
        free(impl);
        mutex->waiters = NULL;
    }
    return XILA_STATUS_OK;
}

static pthread_mutex_t *mutex_impl(XilaMutex *mutex) {
    /* A zero-initialized mutex is usable without mutex_init: initialize lazily. */
    if (mutex->waiters == NULL) {
        pthread_mutex_t *impl = malloc(sizeof *impl);
        pthread_mutexattr_t attr;
        pthread_mutexattr_init(&attr);
        pthread_mutexattr_settype(&attr, mutex->recursive ? PTHREAD_MUTEX_RECURSIVE
                                                          : PTHREAD_MUTEX_NORMAL);
        pthread_mutex_init(impl, &attr);
        pthread_mutexattr_destroy(&attr);
        mutex->waiters = impl;
    }
    return mutex->waiters;
}

static XilaStatus h_mutex_lock(XilaMutex *mutex, uint32_t flags) {
    pthread_mutex_t *impl = mutex_impl(mutex);
    int rc = (flags & XILA_MUTEX_TRY) ? pthread_mutex_trylock(impl) : pthread_mutex_lock(impl);
    if (rc == EBUSY)
        return XILA_STATUS_WOULD_BLOCK;
    if (rc != 0)
        return XILA_STATUS_INVALID_STATE;
    mutex->owner = (size_t)pthread_self();
    mutex->depth++;
    return XILA_STATUS_OK;
}

static XilaStatus h_mutex_unlock(XilaMutex *mutex) {
    mutex->depth--;
    mutex->owner = 0;
    pthread_mutex_unlock(mutex_impl(mutex));
    return XILA_STATUS_OK;
}

static XilaStatus h_random_fill(void *buffer, size_t size) {
    int fd = open("/dev/urandom", O_RDONLY);
    if (fd < 0)
        return XILA_STATUS_INPUT_OUTPUT;
    size_t done = 0;
    while (done < size) {
        ssize_t n = read(fd, (char *)buffer + done, size - done);
        if (n <= 0) {
            close(fd);
            return XILA_STATUS_INPUT_OUTPUT;
        }
        done += (size_t)n;
    }
    close(fd);
    return XILA_STATUS_OK;
}

static const XilaApi api = {
    .version = (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR,
    .size = sizeof(XilaApi),
    .file_read = h_file_read,
    .file_write = h_file_write,
    .file_close = h_file_close,
    .heap_allocate = h_heap_allocate,
    .heap_reallocate = h_heap_reallocate,
    .heap_free = h_heap_free,
    .env_get = h_env_get,
    .env_set = h_env_set,
    .env_remove = h_env_remove,
    .env_entry = h_env_entry,
    .thread_libc = h_thread_libc,
    .thread_current = h_thread_current,
    .thread_has_workers = h_thread_has_workers,
    .thread_stop_others = h_thread_stop_others,
    .mutex_init = h_mutex_init,
    .mutex_destroy = h_mutex_destroy,
    .mutex_lock = h_mutex_lock,
    .mutex_unlock = h_mutex_unlock,
    .random_fill = h_random_fill,
};

/* ---- run scaffolding ---- */
#define HEAP_SIZE (1024 * 1024)

struct run {
    XilaHeap heap;
    XilaProcess process;
    XilaFile in, out, err;
    XilaStartup startup;
    void *region;
    int in_pipe[2], out_pipe[2], err_pipe[2];
};

static void make_pipe(int fds[2]) {
    if (pipe(fds) != 0) {
        perror("pipe");
        exit(2);
    }
}

static const XilaString run_arguments[] = {
    {.data = "-a", .length = 2},
};

static void run_init(struct run *run, const char *env_value) {
    memset(run, 0, sizeof *run);
    run->region = mmap(NULL, HEAP_SIZE, PROT_READ | PROT_WRITE,
                       MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (run->region == MAP_FAILED) {
        perror("mmap");
        exit(2);
    }
    run->heap = (XilaHeap){.base = run->region, .size = HEAP_SIZE, .used = 0};
    run->process = (XilaProcess){.id = 42};
    make_pipe(run->in_pipe);
    make_pipe(run->out_pipe);
    make_pipe(run->err_pipe);
    run->in = (XilaFile){.fd = run->in_pipe[0]};
    run->out = (XilaFile){.fd = run->out_pipe[1]};
    run->err = (XilaFile){.fd = run->err_pipe[1]};
    run->startup = (XilaStartup){
        .size = sizeof(XilaStartup),
        .api = &api,
        .heap = &run->heap,
        .process = &run->process,
        .program_name = {.data = "e2e", .length = 3},
        .arguments = run_arguments,
        .argument_count = 1,
        .standard_input = &run->in,
        .standard_output = &run->out,
        .standard_error = &run->err,
    };
    env_count = 0;
    env_set_internal("XILA_E2E", 8, env_value, strlen(env_value));
}

/* The firmware's ProcessOwner frees the process heap when the run ends; here the region is
   poisoned instead, so any stale pointer into it faults. */
static void run_poison(struct run *run) {
    if (mprotect(run->region, HEAP_SIZE, PROT_NONE) != 0) {
        perror("mprotect");
        exit(2);
    }
}

static void drain(int fd, char *buffer, size_t capacity, size_t *length) {
    size_t total = 0;
    for (;;) {
        ssize_t n = read(fd, buffer + total, capacity - total);
        if (n <= 0)
            break;
        total += (size_t)n;
        if (total == capacity)
            break;
    }
    *length = total;
}

/* Wait until at least `want` bytes arrived on the run's stdout, then keep whatever else
   arrived, so the caller sees the run's whole output. */
static int wait_for_bytes(int fd, size_t want, char *buffer, size_t capacity, size_t *length) {
    size_t total = 0;
    int got_want = 0;
    for (int tries = 0; tries < 500 && !(got_want && total == capacity); ++tries) {
        fd_set set;
        FD_ZERO(&set);
        FD_SET(fd, &set);
        struct timeval tv = {.tv_sec = 0, .tv_usec = 10000};
        if (select(fd + 1, &set, NULL, NULL, &tv) > 0) {
            ssize_t n = read(fd, buffer + total, capacity - total);
            if (n > 0) {
                total += (size_t)n;
                if (total >= want)
                    got_want = 1;
            }
        } else if (got_want) {
            break; /* saw the ready bytes and nothing more arrived */
        }
    }
    *length = total;
    return got_want;
}

struct thread_args {
    struct run *run;
    int32_t status;
};

static void *run_thread(void *arg) {
    struct thread_args *args = arg;
    current_startup = &args->run->startup;
    args->status = my_app(&args->run->startup);
    current_startup = NULL;
    return NULL;
}

static int failures;

static void expect_str(const char *what, const char *got, size_t got_length, const char *want) {
    if (got_length != strlen(want) || memcmp(got, want, got_length) != 0) {
        fprintf(stderr, "FAIL %s:\n  got  %.*s\n  want %s\n", what, (int)got_length, got, want);
        failures++;
    } else {
        fprintf(stderr, "ok %s (%zu bytes)\n", what, got_length);
    }
}

int main(void) {
    char out[4096], err[4096];
    size_t out_len, err_len;

    /* ---- run 1: normal return ---- */
    struct run run1;
    run_init(&run1, "one");
    if (write(run1.in_pipe[1], "line1\n", 6) != 6)
        return 2;
    current_startup = &run1.startup;
    int32_t status1 = my_app(&run1.startup);
    current_startup = NULL;
    run_poison(&run1);
    close(run1.in_pipe[1]);
    close(run1.out_pipe[1]);
    close(run1.err_pipe[1]);
    drain(run1.out_pipe[0], out, sizeof out, &out_len);
    drain(run1.err_pipe[0], err, sizeof err, &err_len);
    if (status1 != 0) {
        fprintf(stderr, "FAIL run 1 status %d\n", status1);
        failures++;
    }
    expect_str("run 1 stdout", out, out_len, "hello one flag=1 err=1 rnd=1\npartialread:line1\n");
    expect_str("run 1 stderr (ready + atexit)", err, err_len, "Ratexit\n");

    /* ---- run 2: cancelled mid-pipe-read ---- */
    struct run run2;
    run_init(&run2, "two");
    /* no input: the entry blocks in fgets; keep the write end open so it stays blocked */
    struct thread_args args = {.run = &run2, .status = -999};
    pthread_t thread;
    if (pthread_create(&thread, NULL, run_thread, &args) != 0)
        return 2;
    size_t ready = 0;
    if (!wait_for_bytes(run2.err_pipe[0], 1, err, sizeof err, &ready)) {
        fprintf(stderr, "FAIL run 2 never signalled ready (%zu bytes)\n", ready);
        failures++;
    }
    /* "cancel": abandon the coroutine (the thread) and free (poison) its heap */
    run_poison(&run2);
    pthread_detach(thread);
    close(run2.out_pipe[1]);
    close(run2.err_pipe[1]);
    err_len = ready;
    drain(run2.out_pipe[0], out, sizeof out, &out_len);
    /* picolibc flushes stdout before a stdin read, so the cancelled run's bytes did
       leave; the relaunch below must not find any of them again. */
    expect_str("run 2 stdout (cancelled mid-read)", out, out_len,
               "hello two flag=1 err=1 rnd=1\npartial");
    expect_str("run 2 stderr (ready signal only)", err, err_len, "R");

    /* ---- run 3: normal return after the cancellation ---- */
    struct run run3;
    run_init(&run3, "three");
    if (write(run3.in_pipe[1], "line3\n", 6) != 6)
        return 2;
    current_startup = &run3.startup;
    int32_t status3 = my_app(&run3.startup);
    current_startup = NULL;
    run_poison(&run3);
    close(run3.in_pipe[1]);
    close(run3.out_pipe[1]);
    close(run3.err_pipe[1]);
    drain(run3.out_pipe[0], out, sizeof out, &out_len);
    drain(run3.err_pipe[0], err, sizeof err, &err_len);
    if (status3 != 0) {
        fprintf(stderr, "FAIL run 3 status %d\n", status3);
        failures++;
    }
    /* Exactly run 3's own output: no leftover buffered bytes of run 2, optind reset so
       getopt sees -a again, and no crash on the freed run 1/run 2 heaps. */
    expect_str("run 3 stdout (after cancellation)", out, out_len,
               "hello three flag=1 err=1 rnd=1\npartialread:line3\n");
    expect_str("run 3 stderr (ready + atexit)", err, err_len, "Ratexit\n");

    if (failures) {
        fprintf(stderr, "%d failure(s)\n", failures);
        return 1;
    }
    fprintf(stderr, "all runs clean\n");
    return 0;
}
