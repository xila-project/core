/* Probe for the behaviours of the SDK's POSIX layer that a hello-world cannot show: what
 * exit and _exit do to buffered streams, the root directory, directory entries and a few
 * argument checks. The first argument selects the scenario; the host test (executable's
 * tests/sdk.rs) launches it once per scenario and inspects the files it leaves behind.
 *
 *   underscore-exit  buffered writes, then _exit: nothing buffered may reach a file
 *   flush-then-exit  fflush(NULL), more writes, an atexit handler, then exit: all of it
 *   environment      POSIX access and raw ABI interoperation
 *   root             open of the root and "." , readdir inodes, errno values
 */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <xila/posix.h>
#include <xila/stubs.h>

#define CHECK(step, condition)                                                           \
    do {                                                                                 \
        if (!(condition)) {                                                              \
            fprintf(stderr, "probe: %s failed: errno %d\n", step, errno);                \
            return 1;                                                                    \
        }                                                                                \
    } while (0)

static FILE *late;

static void at_exit(void) { fputs("handler\n", late); }

static int underscore_exit(void) {
    FILE *stream = fopen("/probe-underscore.txt", "w");
    if (stream == NULL)
        return 1;
    fputs("lost\n", stream);   /* buffered: _exit must not write it */
    fputs("partial", stdout);  /* no newline: stays in the stdout buffer */
    fputs("seen", stderr);     /* unbuffered: reaches the host at once */
    _exit(7);
}

static int flush_then_exit(void) {
    late = fopen("/probe-flush.txt", "w");
    FILE *other = fopen("/probe-flush-other.txt", "w");
    if (late == NULL || other == NULL)
        return 1;
    fputs("one\n", late);
    fputs("g1", other);
    fflush(NULL);          /* flushes both and must leave them tracked for exit */
    fputs("two\n", late);
    fputs("g2", other);
    atexit(at_exit);       /* runs before the final flush: its write must be kept */
    fputs("tail", stdout); /* never terminated by a newline */
    exit(8);
}

static int root(void) {
    struct stat information;
    int fd = open("/", O_RDONLY);
    CHECK("open /", fd >= 0);
    CHECK("fstat /", fstat(fd, &information) == 0 && S_ISDIR(information.st_mode));
    CHECK("close /", close(fd) == 0);
    fd = open(".", O_RDONLY);
    CHECK("open .", fd >= 0);
    CHECK("close .", close(fd) == 0);
    fd = open("/", O_RDONLY | O_DIRECTORY);
    CHECK("open / directory", fd >= 0);
    close(fd);
    CHECK("open / for writing", open("/", O_WRONLY) == -1 && errno == EISDIR);
    CHECK("O_DIRECTORY for writing", open("/", O_WRONLY | O_DIRECTORY) == -1 && errno == EISDIR);
    CHECK("O_DIRECTORY read-write", open("/", O_RDWR | O_DIRECTORY) == -1 && errno == EISDIR);

    /* A directory entry has a nonzero inode, the same one on every pass. */
    CHECK("mkdir", mkdir("/probe-dir", 0755) == 0);
    int made = open("/probe-dir/a", O_WRONLY | O_CREAT, 0644);
    CHECK("create a", made >= 0 && close(made) == 0);
    made = open("/probe-dir/b", O_WRONLY | O_CREAT, 0644);
    CHECK("create b", made >= 0 && close(made) == 0);
    DIR *directory = opendir("/probe-dir");
    CHECK("opendir", directory != NULL);
    unsigned long long first[2] = {0, 0};
    int count = 0;
    for (struct dirent *entry; (entry = readdir(directory)) != NULL; ++count) {
        CHECK("d_ino", entry->d_ino != 0 && count < 2);
        first[count] = (unsigned long long)entry->d_ino;
    }
    CHECK("two entries", count == 2 && first[0] != first[1]);
    CHECK("64-bit inodes", ((first[0] | first[1]) >> 32) != 0);
    rewinddir(directory);
    for (int index = 0; index < 2; ++index) {
        struct dirent *entry = readdir(directory);
        CHECK("again", entry != NULL && (unsigned long long)entry->d_ino == first[index]);
    }
    CHECK("closedir", closedir(directory) == 0);

    /* openat stays inside its directory; the errno is the one Core's escape status gets. */
    int inside = open("/probe-dir", O_RDONLY | O_DIRECTORY);
    CHECK("open dir", inside >= 0);
    CHECK("openat escape", openat(inside, "../probe-dir/a", O_RDONLY) == -1 && errno == EACCES);
    int ok = openat(inside, "a", O_RDONLY);
    CHECK("openat inside", ok >= 0 && close(ok) == 0);
    CHECK("close dir", close(inside) == 0);
    CHECK("unlink a", unlink("/probe-dir/a") == 0 && unlink("/probe-dir/b") == 0);
    CHECK("rmdir", rmdir("/probe-dir") == 0);

    unsigned char random[257];
    CHECK("getentropy 257", getentropy(random, sizeof(random)) == -1 && errno == EINVAL);

    puts("probe: ok");
    return 0;
}

static void *write_pipe(void *arg) {
    int fd = *(int *)arg;
    if (write(fd, "ok", 2) != 2 || close(fd) != 0)
        return NULL;
    return arg;
}

struct drain_pipe {
    int fd;
    unsigned char data[XILA_PIPE_CAPACITY];
};

static void *drain_pipe(void *arg) {
    struct drain_pipe *drain = arg;
    if (read(drain->fd, drain->data, 112) != 112 || sched_yield() != 0 ||
        read(drain->fd, drain->data + 112, sizeof(drain->data) - 112) !=
            (ssize_t)(sizeof(drain->data) - 112))
        return NULL;
    return arg;
}

struct pipe_record {
    int fd;
    unsigned char bytes[300];
};

static void *write_record(void *arg) {
    struct pipe_record *record = arg;
    return write(record->fd, record->bytes, sizeof(record->bytes)) ==
                   (ssize_t)sizeof(record->bytes)
               ? arg : NULL;
}

static int atomic_pipe_writes(void) {
    int fds[2];
    CHECK("atomic pipe", pipe(fds) == 0);
    int duplicate = dup(fds[1]);
    CHECK("duplicate pipe writer", duplicate >= 0);
    struct pipe_record first = {.fd = fds[1]}, second = {.fd = duplicate};
    memset(first.bytes, 'a', sizeof(first.bytes));
    memset(second.bytes, 'b', sizeof(second.bytes));
    unsigned char filler[400] = {0};
    CHECK("leave too little free space for a record",
          write(fds[1], filler, sizeof(filler)) == (ssize_t)sizeof(filler));

    pthread_t writers[2];
    CHECK("start record writers", pthread_create(&writers[0], NULL, write_record, &first) == 0 &&
          pthread_create(&writers[1], NULL, write_record, &second) == 0);
    CHECK("allow both writers to block", sched_yield() == 0);
    int queued = 0;
    CHECK("both writers left the pipe intact", ioctl(fds[0], FIONREAD, &queued) == 0 &&
          queued == (int)sizeof(filler));
    CHECK("drain the filler", read(fds[0], filler, sizeof(filler)) == (ssize_t)sizeof(filler));

    unsigned char records[sizeof(first.bytes) + sizeof(second.bytes)];
    for (size_t offset = 0; offset < sizeof(records);) {
        struct pollfd input = {fds[0], POLLIN, 0};
        CHECK("wait for a record", poll(&input, 1, 1000) == 1 && input.revents == POLLIN);
        ssize_t count = read(fds[0], records + offset, sizeof(records) - offset);
        CHECK("read a record", count == (ssize_t)sizeof(first.bytes));
        offset += (size_t)count;
    }
    void *result = NULL;
    CHECK("both record writes completed in one call",
          pthread_join(writers[0], &result) == 0 && result == &first &&
          pthread_join(writers[1], &result) == 0 && result == &second);
    CHECK("records did not interleave",
          (memcmp(records, first.bytes, sizeof(first.bytes)) == 0 &&
           memcmp(records + sizeof(first.bytes), second.bytes, sizeof(second.bytes)) == 0) ||
          (memcmp(records, second.bytes, sizeof(second.bytes)) == 0 &&
           memcmp(records + sizeof(second.bytes), first.bytes, sizeof(first.bytes)) == 0));
    CHECK("close atomic pipe", close(fds[0]) == 0 && close(fds[1]) == 0 && close(duplicate) == 0);
    return 0;
}

static int64_t monotonic_ns(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        return -1;
    return (int64_t)now.tv_sec * 1000000000 + now.tv_nsec;
}

static int descriptors(void) {
    int fd = open("/positional.txt", O_CREAT | O_TRUNC | O_RDWR | O_CLOEXEC, 0666);
    CHECK("create file", fd >= 0 && write(fd, "abcd", 4) == 4);
    CHECK("offset reset", lseek(fd, 0, SEEK_SET) == 0);
    CHECK("flags on open", fcntl(fd, F_GETFD) == FD_CLOEXEC);
    int copy = dup(fd);
    CHECK("lowest dup slot", copy == fd + 1 && fcntl(copy, F_GETFD) == 0);
    int high = fcntl(fd, F_DUPFD_CLOEXEC, 20);
    CHECK("dup minimum and flag", high == 20 && fcntl(high, F_GETFD) == FD_CLOEXEC);
    CHECK("dup2 with same fd preserves flag", dup2(high, high) == high &&
          fcntl(high, F_GETFD) == FD_CLOEXEC);
    CHECK("dup2 invalid source preserves destination", dup2(255, high) == -1 &&
          errno == EBADF && fcntl(high, F_GETFD) == FD_CLOEXEC);
    CHECK("dup2 clears flag", dup2(fd, high) == high && fcntl(high, F_GETFD) == 0);
    CHECK("file status flags exclude descriptor flags", fcntl(fd, F_GETFL) == O_RDWR);
    CHECK("status flags shared by duplicates",
          fcntl(copy, F_SETFL, O_RDONLY | O_NONBLOCK | O_APPEND) == 0 &&
          fcntl(fd, F_GETFL) == (O_RDWR | O_NONBLOCK | O_APPEND) &&
          fcntl(high, F_GETFL) == (O_RDWR | O_NONBLOCK | O_APPEND) &&
          fcntl(fd, F_GETFD) == FD_CLOEXEC);
    CHECK("clear shared status flags", fcntl(high, F_SETFL, 0) == 0 &&
          fcntl(copy, F_GETFL) == O_RDWR);

    char data[8] = {0};
    CHECK("read updates shared offset", read(copy, data, 1) == 1 && data[0] == 'a' &&
          lseek(high, 0, SEEK_CUR) == 1);
    CHECK("pread short at EOF", pread(high, data, sizeof(data), 2) == 2 &&
          memcmp(data, "cd", 2) == 0 && lseek(fd, 0, SEEK_CUR) == 1);
    CHECK("pwrite leaves offset", pwrite(high, "xy", 2, 2) == 2 &&
          lseek(copy, 0, SEEK_CUR) == 1);
    CHECK("zero-length positional I/O", pread(fd, NULL, 0, 0) == 0 &&
          pwrite(fd, NULL, 0, 0) == 0);
    CHECK("positional invalid offset", pread(fd, data, 1, -1) == -1 && errno == EINVAL);
    CHECK("dups survive close", close(fd) == 0 && read(copy, data, sizeof(data)) == 3 &&
          memcmp(data, "bxy", 3) == 0);
    struct pollfd file = {copy, POLLIN | POLLOUT, 0};
    CHECK("file ready at EOF", poll(&file, 1, 0) == 1 && file.revents == (POLLIN | POLLOUT));
    CHECK("write appends after setting O_APPEND on a duplicate",
          fcntl(high, F_SETFL, O_APPEND) == 0 && lseek(copy, 0, SEEK_SET) == 0 &&
          write(copy, "!", 1) == 1 && lseek(copy, 0, SEEK_CUR) == 5 &&
          pread(high, data, sizeof(data), 0) == 5 && memcmp(data, "abxy!", 5) == 0);
    CHECK("regular file is not a terminal", isatty(copy) == 0 && errno == ENOTTY);
    CHECK("isatty invalid fd", isatty(fd) == 0 && errno == EBADF);
    CHECK("close copies", close(copy) == 0 && close(high) == 0);

    int write_only = open("/positional.txt", O_WRONLY);
    CHECK("open write-only file", write_only >= 0);
    file = (struct pollfd){write_only, POLLIN | POLLOUT, 0};
    CHECK("write-only file reports only output readiness",
          poll(&file, 1, 0) == 1 && file.revents == POLLOUT);
    file.events = POLLIN;
    CHECK("write-only file has no input readiness", poll(&file, 1, 0) == 0 && file.revents == 0);
    CHECK("close write-only file", close(write_only) == 0);

    int directory = open("/", O_RDONLY | O_DIRECTORY);
    int directory_copy = fcntl(directory, F_DUPFD, 17);
    CHECK("duplicate directory", directory == fd && directory_copy == 17 &&
          close(directory) == 0);
    int relative = openat(directory_copy, "positional.txt", O_RDONLY);
    CHECK("duplicated directory is usable", relative == directory && close(relative) == 0 &&
          close(directory_copy) == 0);

    int pipefd[2];
    CHECK("pipe2 flags", pipe2(pipefd, O_NONBLOCK | O_CLOEXEC) == 0 &&
          fcntl(pipefd[0], F_GETFD) == FD_CLOEXEC && fcntl(pipefd[1], F_GETFD) == FD_CLOEXEC);
    CHECK("pipe status and access flags",
          fcntl(pipefd[0], F_GETFL) == (O_RDONLY | O_NONBLOCK) &&
          fcntl(pipefd[1], F_GETFL) == (O_WRONLY | O_NONBLOCK));
    int reader_copy = dup(pipefd[0]);
    CHECK("clear nonblocking on the duplicate",
          reader_copy >= 0 && fcntl(reader_copy, F_SETFL, 0) == 0 &&
          fcntl(pipefd[0], F_GETFL) == O_RDONLY &&
          fcntl(pipefd[1], F_GETFL) == (O_WRONLY | O_NONBLOCK));
    CHECK("set nonblocking on the original", fcntl(pipefd[0], F_SETFL, O_NONBLOCK) == 0 &&
          fcntl(reader_copy, F_GETFL) == (O_RDONLY | O_NONBLOCK));
    unsigned char byte;
    CHECK("shared nonblocking affects reads", read(reader_copy, &byte, 1) == -1 && errno == EAGAIN);
    CHECK("close duplicate reader", close(reader_copy) == 0);

    struct pollfd waiting[] = {{pipefd[0], POLLIN, 0}, {-1, POLLIN, 1}, {255, 0, 0}};
    errno = EDOM;
    CHECK("invalid and ignored entries", poll(waiting, 3, 0) == 1 && errno == EDOM &&
          waiting[0].revents == 0 && waiting[1].revents == 0 && waiting[2].revents == POLLNVAL);
    unsigned char big[XILA_PIPE_CAPACITY + 8] = {0};
    CHECK("short write at capacity", write(pipefd[1], big, sizeof(big)) == XILA_PIPE_CAPACITY);
    CHECK("write would block", write(pipefd[1], big, 1) == -1 && errno == EAGAIN);
    struct pollfd output = {pipefd[1], POLLOUT, 0};
    CHECK("full pipe not writable", poll(&output, 1, 0) == 0 && output.revents == 0);
    int available = 0;
    CHECK("FIONREAD", ioctl(pipefd[0], FIONREAD, &available) == 0 &&
          available == XILA_PIPE_CAPACITY);
    CHECK("drain just 112 bytes", read(pipefd[0], big, 112) == 112);
    CHECK("partial space is not writable", poll(&output, 1, 0) == 0 && output.revents == 0);
    CHECK("200-byte write needs the whole record's space",
          write(pipefd[1], big, 200) == -1 && errno == EAGAIN);
    CHECK("failed atomic write queued nothing", ioctl(pipefd[0], FIONREAD, &available) == 0 &&
          available == 400);
    CHECK("unsupported ioctl", ioctl(pipefd[0], TIOCLINUX, NULL) == -1 && errno == ENOTTY);
    CHECK("pipe lseek ESPIPE", lseek(pipefd[0], 0, SEEK_SET) == -1 && errno == ESPIPE);
    CHECK("pipe pread ESPIPE", pread(pipefd[0], big, 1, 0) == -1 && errno == ESPIPE);
    CHECK("drain pipe", read(pipefd[0], big, sizeof(big)) == 400);
    CHECK("space makes pipe writable", poll(&output, 1, 0) == 1 && output.revents == POLLOUT);
    int64_t before = monotonic_ns();
    CHECK("empty pipe finite timeout", before >= 0 && poll(waiting, 1, 5) == 0 &&
          waiting[0].revents == 0 && monotonic_ns() - before >= 5000000);
    before = monotonic_ns();
    CHECK("empty descriptor set sleeps", before >= 0 && poll(NULL, 0, 3) == 0 &&
          monotonic_ns() - before >= 3000000);
    CHECK("close reader", close(pipefd[0]) == 0);
    CHECK("broken pipe readiness", poll(&output, 1, 0) == 1 && output.revents == POLLERR);
    CHECK("write to broken pipe", write(pipefd[1], "x", 1) == -1 && errno == EPIPE);
    CHECK("close writer", close(pipefd[1]) == 0);

    CHECK("blocking pipe", pipe(pipefd) == 0);
    int writer_copy = dup(pipefd[1]);
    CHECK("duplicate keeps pipe writer alive", writer_copy >= 0 && close(pipefd[1]) == 0);
    waiting[0] = (struct pollfd){pipefd[0], POLLIN, 0};
    CHECK("no hangup while writer copy is open", poll(waiting, 1, 0) == 0);
    int other[2];
    CHECK("second pipe", pipe(other) == 0);
    waiting[1] = (struct pollfd){other[0], POLLIN, 0};
    pthread_t writer;
    CHECK("spawn writer", pthread_create(&writer, NULL, write_pipe, &writer_copy) == 0);
    CHECK("peer wakes blocking multi-handle poll", poll(waiting, 2, 1000) == 1 &&
          (waiting[0].revents & POLLIN) && waiting[1].revents == 0);
    void *result = NULL;
    CHECK("join writer", pthread_join(writer, &result) == 0 && result == &writer_copy);
    CHECK("remote close after write", poll(waiting, 2, 0) == 1 &&
          waiting[0].revents == (POLLIN | POLLHUP) && waiting[1].revents == 0);
    CHECK("close second pipe", close(other[0]) == 0 && close(other[1]) == 0);
    CHECK("consume data after hangup", read(pipefd[0], data, sizeof(data)) == 2 &&
          memcmp(data, "ok", 2) == 0 && read(pipefd[0], data, sizeof(data)) == 0);
    CHECK("close reader", close(pipefd[0]) == 0);

    CHECK("create pipe for blocking POLLOUT", pipe(pipefd) == 0);
    CHECK("fill pipe for POLLOUT", write(pipefd[1], big, XILA_PIPE_CAPACITY) == XILA_PIPE_CAPACITY);
    struct drain_pipe drain = {.fd = pipefd[0]};
    pthread_t drainer;
    CHECK("start draining peer", pthread_create(&drainer, NULL, drain_pipe, &drain) == 0);
    output = (struct pollfd){pipefd[1], POLLOUT, 0};
    CHECK("blocking POLLOUT wakes after the pipe drains",
          poll(&output, 1, 1000) == 1 && output.revents == POLLOUT &&
          ioctl(pipefd[0], FIONREAD, &available) == 0 && available == 0);
    CHECK("join draining peer", pthread_join(drainer, &result) == 0 && result == &drain);
    CHECK("close drained pipe", close(pipefd[0]) == 0 && close(pipefd[1]) == 0);
    int records = atomic_pipe_writes();
    if (records != 0)
        return records;

    puts("descriptors: ok");
    return 0;
}

/* Capture the raw capability only for the interoperation tests. */
static const XilaProcess *process;
/* Count live SDK allocations across raw rewrites and inject guest heap failures. */
static size_t live_blocks;
static bool fail_allocation;
static bool shrank_probe;
static bool retried_entry;

XilaStatus __real_xila_env_entry(const XilaProcess *, size_t, char *, size_t, size_t *);

XilaStatus __wrap_xila_env_entry(const XilaProcess *capability, size_t index, char *buffer,
                                size_t capacity, size_t *length) {
    XilaStatus status = __real_xila_env_entry(capability, index, buffer, capacity, length);
    if (status == XILA_STATUS_BUFFER_TOO_SMALL && index == 0) {
        if (buffer == NULL && !shrank_probe && *length > 1) {
            /* Simulate an entry that grows before startup copies it. */
            --*length;
            shrank_probe = true;
        } else if (buffer != NULL && shrank_probe) {
            retried_entry = true;
        }
    }
    return status;
}

XilaStatus __real_xila_heap_allocate(XilaHeap *, size_t, void **);
XilaStatus __real_xila_heap_reallocate(XilaHeap *, void *, size_t, void **);
XilaStatus __real_xila_heap_free(XilaHeap *, void *);

XilaStatus __wrap_xila_heap_allocate(XilaHeap *heap, size_t size, void **allocation) {
    if (fail_allocation)
        return XILA_STATUS_OUT_OF_MEMORY;
    XilaStatus status = __real_xila_heap_allocate(heap, size, allocation);
    if (status == XILA_STATUS_OK)
        ++live_blocks;
    return status;
}

XilaStatus __wrap_xila_heap_reallocate(XilaHeap *heap, void *allocation, size_t size, void **output) {
    XilaStatus status = __real_xila_heap_reallocate(heap, allocation, size, output);
    if (status == XILA_STATUS_OK && allocation == NULL)
        ++live_blocks;
    return status;
}

XilaStatus __wrap_xila_heap_free(XilaHeap *heap, void *allocation) {
    XilaStatus status = __real_xila_heap_free(heap, allocation);
    if (status == XILA_STATUS_OK && allocation != NULL)
        --live_blocks;
    return status;
}

int32_t __real_xila_main(const XilaStartup *startup);

int32_t __wrap_xila_main(const XilaStartup *startup) {
    process = startup->process;
    return __real_xila_main(startup);
}

static void *environment_worker(void *expected) {
    if (getenv("SDK_PROBE") == expected && unsetenv("") == -1 && errno == EINVAL)
        return expected;
    return NULL;
}

static const char *environment_entry(const char *name) {
    size_t length = strlen(name);
    for (char **entry = environ; *entry != NULL; ++entry)
        if (strncmp(*entry, name, length) == 0 && (*entry)[length] == '=')
            return *entry + length + 1;
    return NULL;
}

static bool environment_value_is(const char *name, const char *expected) {
    const char *value = environment_entry(name);
    return value != NULL && strcmp(value, expected) == 0;
}

static int environment(const char *program) {
    CHECK("retry a growing startup entry", retried_entry);
    CHECK("iterate inherited environ",
          environ != NULL && environment_value_is("SDK_PROBE", "inherited") &&
          environment_value_is("SDK_OTHER", "a=b"));
    uint32_t id = 0, parent = 0;
    CHECK("getpid reads the process capability",
          xila_process_id(process, &id) == XILA_STATUS_OK && getpid() == (pid_t)id);
    CHECK("getppid reads the process capability",
          xila_process_parent_id(process, &parent) == XILA_STATUS_OK && getppid() == (pid_t)parent);

    const char *name = "SDK_PROBE";
    char *initial = getenv(name);
    CHECK("inherited environment", initial != NULL && strcmp(initial, "inherited") == 0);
    CHECK("program name", strcmp(program, "probe") == 0);
    errno = EDOM;
    CHECK("missing value preserves errno", getenv("SDK_MISSING") == NULL && errno == EDOM);
    pthread_t worker;
    void *worker_value = NULL;
    CHECK("start environment reader", pthread_create(&worker, NULL, environment_worker, initial) == 0);
    CHECK("worker sees the same value and has its own errno",
          pthread_join(worker, &worker_value) == 0 && worker_value == initial && errno == EDOM);
    CHECK("existing value without overwrite", setenv(name, "ignored", 0) == 0 &&
          getenv(name) == initial);

    CHECK("set empty value", setenv("SDK_EMPTY", "", 0) == 0);
    CHECK("environ includes new empty value", environment_value_is("SDK_EMPTY", ""));
    const char *empty = getenv("SDK_EMPTY");
    CHECK("get empty value", empty != NULL && *empty == '\0');
    CHECK("empty value counts as present", setenv("SDK_EMPTY", "ignored", 0) == 0 &&
          getenv("SDK_EMPTY") == empty);
    CHECK("unrelated set and get preserve the pointer", strcmp(initial, "inherited") == 0);

    CHECK("overwrite", setenv(name, "new", 1) == 0);
    CHECK("environ includes overwritten value", environment_value_is(name, "new"));
    const char *current = getenv(name);
    CHECK("read overwritten value", current != NULL && strcmp(current, "new") == 0);
    char raw[3];
    size_t length;
    CHECK("POSIX change visible to ABI",
          xila_env_get(process, name, strlen(name), raw, sizeof(raw), &length) == XILA_STATUS_OK &&
          length == sizeof(raw) && memcmp(raw, "new", sizeof(raw)) == 0);

    size_t cached_blocks = live_blocks;
    char large[513];
    memset(large, 'x', sizeof(large) - 1);
    large[sizeof(large) - 1] = '\0';
    CHECK("set long value through ABI",
          xila_env_set(process, name, strlen(name), large, strlen(large)) == XILA_STATUS_OK);
    const char *long_value = getenv(name);
    CHECK("get long value with terminator", long_value != NULL && strcmp(long_value, large) == 0);
    CHECK("replacing a cached value keeps one allocation per name", live_blocks == cached_blocks);
    CHECK("repeated long lookup", getenv(name) == long_value);
    for (char byte = 'a'; byte <= 'z'; ++byte) {
        large[0] = byte;
        CHECK("rewrite through ABI",
              xila_env_set(process, name, strlen(name), large, strlen(large)) == XILA_STATUS_OK);
        const char *updated = getenv(name);
        CHECK("get rewritten value", updated != NULL && strcmp(updated, large) == 0);
        CHECK("raw rewrites do not grow the cache", live_blocks == cached_blocks);
        CHECK("other name retains its cached pointer", getenv("SDK_EMPTY") == empty);
    }
    CHECK("raw removal is visible",
          xila_env_remove(process, name, strlen(name)) == XILA_STATUS_OK && getenv(name) == NULL);
    CHECK("raw removal releases the cached value", live_blocks == cached_blocks - 1);
    CHECK("unset absent name", unsetenv(name) == 0);

    CHECK("set after removal", setenv(name, "again", 0) == 0);
    CHECK("unset present name", unsetenv(name) == 0 && getenv(name) == NULL);
    CHECK("ABI sees unset",
          xila_env_get(process, name, strlen(name), raw, sizeof(raw), &length) == XILA_STATUS_NOT_FOUND);
    CHECK("environ no longer includes unset name", environment_entry(name) == NULL);
    char assignment[] = "SDK_EMPTY=put";
    CHECK("putenv updates through setenv", putenv(assignment) == 0);
    assignment[sizeof(assignment) - 2] = 'x';
    CHECK("environ includes copied putenv value", environment_value_is("SDK_EMPTY", "put"));
    const char *put = getenv("SDK_EMPTY");
    CHECK("get putenv value", put != NULL && strcmp(put, "put") == 0);
    CHECK("invalid set name", setenv("A=B", "bad", 0) == -1 && errno == EINVAL);
    errno = 0;
    CHECK("empty set name", setenv("", "bad", 1) == -1 && errno == EINVAL);
    errno = 0;
    CHECK("invalid get name", getenv("A=B") == NULL && errno == EINVAL);
    errno = 0;
    CHECK("invalid unset name", unsetenv("") == -1 && errno == EINVAL);
    errno = 0;
    CHECK("NULL set name", setenv(NULL, "bad", 1) == -1 && errno == EINVAL);
    errno = 0;
    CHECK("NULL unset name", unsetenv(NULL) == -1 && errno == EINVAL);
    CHECK("set prefix names", setenv("A", "a", 1) == 0 && setenv("AB", "ab", 1) == 0);
    CHECK("environ contains both prefix names",
          environment_value_is("A", "a") && environment_value_is("AB", "ab"));
    const char *ab = getenv("AB");
    const char *a = getenv("A");
    CHECK("look up prefix names independently",
          ab != NULL && strcmp(ab, "ab") == 0 && a != NULL && strcmp(a, "a") == 0);
    CHECK("unset prefix while retaining the longer name",
          unsetenv("A") == 0 && getenv("A") == NULL && getenv("AB") == ab);
    CHECK("environ distinguishes prefix names after removal",
          environment_entry("A") == NULL && environment_value_is("AB", "ab"));
    fail_allocation = true;
    errno = 0;
    int result = setenv("AB", "cannot store", 1);
    int update_errno = errno;
    errno = 0;
    const char *uncached = getenv("SDK_OTHER");
    int allocation_errno = errno;
    fail_allocation = false;
    CHECK("environ and the host preserve the old value after an allocation failure",
          result == -1 && update_errno == ENOMEM && environment_value_is("AB", "ab") &&
          getenv("AB") == ab);
    CHECK("getenv allocation failure", uncached == NULL && allocation_errno == ENOMEM);

    char **before_last_unset = environ;
    CHECK("remove all table entries",
          unsetenv("SDK_GREETING") == 0 && unsetenv("SDK_OTHER") == 0 &&
          unsetenv("SDK_EMPTY") == 0 && unsetenv("AB") == 0 &&
          environ == before_last_unset && before_last_unset[0] == NULL);
    CHECK("add after emptying the table",
          setenv("A", "again", 1) == 0 && environment_value_is("A", "again") &&
          environ[1] == NULL);

    CHECK("write a host-only variable",
          xila_env_set(process, "RAW", 3, "present", 7) == XILA_STATUS_OK);
    static char *custom[] = {
        "SDK_CUSTOM=a=b", "malformed", "SDK_CUSTOM=later", "SDK_STATIC=keep", NULL,
    };
    char **saved = environ;
    environ = custom;
    const char *temporary = getenv("SDK_CUSTOM");
    CHECK("getenv adopts the temporary table", temporary != NULL && strcmp(temporary, "a=b") == 0);
    environ = saved;
    const char *restored = getenv("A");
    CHECK("getenv adopts a saved SDK table",
          restored != NULL && strcmp(restored, "again") == 0 &&
          environment_value_is("A", "again") && getenv("SDK_CUSTOM") == NULL);
    environ = custom;
    const char *adopted = getenv("SDK_CUSTOM");
    CHECK("getenv reads the assigned environment", adopted != NULL && strcmp(adopted, "a=b") == 0);
    CHECK("getenv publishes an SDK-owned copy", environ != custom);
    CHECK("adoption removes the old environment", getenv("A") == NULL && getenv("RAW") == NULL);
    CHECK("adoption keeps valid entries and skips malformed ones",
          environment_value_is("SDK_STATIC", "keep") && environ[2] == NULL);
    CHECK("replace and remove entries after adopting a static environ",
          setenv("SDK_CUSTOM", "updated", 1) == 0 && unsetenv("SDK_STATIC") == 0 &&
          environment_value_is("SDK_CUSTOM", "updated") && environ[1] == NULL &&
          strcmp(custom[0], "SDK_CUSTOM=a=b") == 0 && strcmp(custom[3], "SDK_STATIC=keep") == 0);
    static char *invalid[] = {
        "SDK_INVALID=\xff", "SDK_OVERLONG=\xe0\x80\x80", "SDK_SURROGATE=\xed\xa0\x80",
        "SDK_TOO_HIGH=\xf4\x90\x80\x80", "SDK_TRUNCATED=\xc2", "\xff=value",
        "SDK_UTF8=\xc3\xa9\xe2\x82\xac\xf0\x9f\x98\x80", NULL,
    };
    environ = invalid;
    const char *utf8 = getenv("SDK_UTF8");
    CHECK("adoption skips invalid UTF-8 and installs the valid entries",
          utf8 != NULL && strcmp(utf8, "\xc3\xa9\xe2\x82\xac\xf0\x9f\x98\x80") == 0 &&
          environment_value_is("SDK_UTF8", utf8) && environ[1] == NULL &&
          getenv("SDK_INVALID") == NULL && getenv("SDK_CUSTOM") == NULL);

    static char *remove_first[] = {"SDK_REMOVE=present", NULL};
    environ = remove_first;
    CHECK("unsetenv adopts before removing the name",
          unsetenv("SDK_REMOVE") == 0 && environ[0] == NULL && getenv("SDK_UTF8") == NULL);
    environ = NULL;
    CHECK("adopting another table discards the oldest table", getenv("SDK_REMOVE") == NULL);
    size_t empty_blocks = live_blocks;

    char **allocated = malloc(2 * sizeof(*allocated));
    char *allocated_entry = strdup("SDK_ALLOCATED=original");
    CHECK("allocate a caller-owned environ", allocated != NULL && allocated_entry != NULL);
    allocated[0] = allocated_entry;
    allocated[1] = NULL;
    environ = allocated;
    fail_allocation = true;
    int adoption_result = setenv("SDK_ALLOCATED", "ignored", 0);
    fail_allocation = false;
    CHECK("allocation failure leaves the assigned table available for retry",
          adoption_result == -1 && errno == ENOMEM && environ == allocated &&
          live_blocks == empty_blocks + 2);
    CHECK("setenv adopts before checking overwrite",
          setenv("SDK_ALLOCATED", "ignored", 0) == 0 && environ != allocated &&
          environment_value_is("SDK_ALLOCATED", "original"));
    CHECK("only the caller owns the supplied table and string",
          allocated[0] == allocated_entry && allocated[1] == NULL &&
          strcmp(allocated_entry, "SDK_ALLOCATED=original") == 0);
    free(allocated_entry);
    free(allocated);

    environ = NULL;
    CHECK("setenv adopts a NULL environ",
          setenv("SDK_AFTER_CLEAR", "present", 1) == 0 &&
          environment_value_is("SDK_AFTER_CLEAR", "present"));
    const char *after_clear = getenv("SDK_AFTER_CLEAR");
    CHECK("NULL assignment clears the host environment",
          after_clear != NULL && strcmp(after_clear, "present") == 0 &&
          getenv("SDK_ALLOCATED") == NULL);
    CHECK("only the current empty array and one previous table remain allocated",
          unsetenv("SDK_AFTER_CLEAR") == 0 && environ[0] == NULL &&
          live_blocks == empty_blocks + 2);
    environ = NULL;
    CHECK("another adoption frees the previous table",
          getenv("SDK_AFTER_CLEAR") == NULL && live_blocks == empty_blocks);

    puts("environment: ok");
    return 0;
}

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    if (strcmp(mode, "underscore-exit") == 0)
        return underscore_exit();
    if (strcmp(mode, "flush-then-exit") == 0)
        return flush_then_exit();
    if (strcmp(mode, "root") == 0)
        return root();
    if (strcmp(mode, "environment") == 0)
        return environment(argv[0]);
    if (strcmp(mode, "descriptors") == 0)
        return descriptors();
    fprintf(stderr, "probe: unknown scenario \"%s\"\n", mode);
    return 2;
}
