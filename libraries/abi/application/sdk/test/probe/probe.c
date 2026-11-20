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
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/stat.h>
#include <unistd.h>

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

int main(int argc, char **argv) {
    const char *mode = argc > 1 ? argv[1] : "";
    if (strcmp(mode, "underscore-exit") == 0)
        return underscore_exit();
    if (strcmp(mode, "flush-then-exit") == 0)
        return flush_then_exit();
    if (strcmp(mode, "root") == 0)
        return root();
    fprintf(stderr, "probe: unknown scenario \"%s\"\n", mode);
    return 2;
}
