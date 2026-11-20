/* Probe for the behaviours of the SDK's POSIX layer that a hello-world cannot show: what
 * exit and _exit do to buffered streams, the root directory, directory entries and a few
 * argument checks. The first argument selects the scenario; the host test (executable's
 * tests/sdk.rs) launches it once per scenario and inspects the files it leaves behind.
 *
 *   underscore-exit  buffered writes, then _exit: nothing buffered may reach a file
 *   flush-then-exit  fflush(NULL), more writes, an atexit handler, then exit: all of it
 *   root             open of the root and "." , readdir inodes, errno values
 */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#include <xila/application.h>

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
    CHECK("regular file is not a terminal", isatty(copy) == 0 && errno == ENOTTY);
    CHECK("isatty invalid fd", isatty(fd) == 0 && errno == EBADF);
    CHECK("close copies", close(copy) == 0 && close(high) == 0);

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
    CHECK("unsupported ioctl", ioctl(pipefd[0], TIOCLINUX, NULL) == -1 && errno == ENOTTY);
    CHECK("pipe lseek ESPIPE", lseek(pipefd[0], 0, SEEK_SET) == -1 && errno == ESPIPE);
    CHECK("pipe pread ESPIPE", pread(pipefd[0], big, 1, 0) == -1 && errno == ESPIPE);
    CHECK("drain pipe", read(pipefd[0], big, sizeof(big)) == XILA_PIPE_CAPACITY);
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

    puts("descriptors: ok");
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
    if (strcmp(mode, "descriptors") == 0)
        return descriptors();
    fprintf(stderr, "probe: unknown scenario \"%s\"\n", mode);
    return 2;
}
