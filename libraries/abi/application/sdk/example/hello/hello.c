/* Hello world for Xila native applications, built against the SDK with CMake.
 *
 * It exercises the C library and the POSIX layer on top of the native ABI: stdio,
 * files and directories, the environment, clocks and entropy. On success it returns 0 and prints
 * "hello: ok"; on failure it names the step and returns a non-zero code. */
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/random.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#include <xila/posix.h>

#define CHECK(step, condition)                                                           \
    do {                                                                                 \
        if (!(condition)) {                                                              \
            fprintf(stderr, "hello: %s failed: errno %d\n", step, errno);                \
            return 1;                                                                    \
        }                                                                                \
    } while (0)

int main(int argc, char **argv) {
    printf("hello from Xila, %d argument(s)\n", argc - 1);

    const char *greeting = getenv("SDK_GREETING");
    if (greeting != NULL)
        printf("greeting: %s\n", greeting);

    /* Standard C: formatted output without float support, and the heap. */
    char *text = malloc(32);
    CHECK("malloc", text != NULL);
    snprintf(text, 32, "%d-%s", 42, "xila");
    CHECK("snprintf", strcmp(text, "42-xila") == 0);
    free(text);
#ifdef HELLO_FLOAT
    /* Built with -DXILA_PRINTF_FORMAT=double. */
    char number[16];
    snprintf(number, sizeof(number), "%.2f", 2.5 * 3.0);
    CHECK("printf float", strcmp(number, "7.50") == 0);
#endif

    /* Files and directories. Paths are resolved against the granted directory. */
    CHECK("mkdir", mkdir("/sdk-hello", 0755) == 0 || errno == EEXIST);
    CHECK("chdir", chdir("/sdk-hello") == 0);
    char cwd[32];
    CHECK("getcwd", getcwd(cwd, sizeof(cwd)) != NULL && strcmp(cwd, "/sdk-hello") == 0);

    int fd = open("note.txt", O_RDWR | O_CREAT | O_TRUNC, 0644);
    CHECK("open", fd >= 0);
    CHECK("write", write(fd, "hello, files", 12) == 12);
    CHECK("lseek", lseek(fd, 7, SEEK_SET) == 7);
    char word[8] = {0};
    CHECK("read", read(fd, word, 5) == 5 && strcmp(word, "files") == 0);
    CHECK("pread", pread(fd, word, 5, 0) == 5 && memcmp(word, "hello", 5) == 0);
    CHECK("cursor kept", lseek(fd, 0, SEEK_CUR) == 12);
    struct stat information;
    CHECK("fstat", fstat(fd, &information) == 0 && information.st_size == 12 &&
                       S_ISREG(information.st_mode));
    CHECK("close", close(fd) == 0);

    CHECK("stat", stat("/sdk-hello/note.txt", &information) == 0 && information.st_size == 12);
    CHECK("rename", rename("note.txt", "renamed.txt") == 0);
    CHECK("stat missing", stat("note.txt", &information) != 0 && errno == ENOENT);

    FILE *stream = fopen("stream.txt", "w");
    CHECK("fopen", stream != NULL);
    CHECK("fprintf", fprintf(stream, "%s %ld\n", "line", 7L) > 0);
    CHECK("fclose", fclose(stream) == 0);
    stream = fopen("stream.txt", "r");
    CHECK("fopen read", stream != NULL);
    char line[16];
    CHECK("fgets", fgets(line, sizeof(line), stream) != NULL && strcmp(line, "line 7\n") == 0);
    fclose(stream);

    DIR *directory = opendir("/sdk-hello");
    CHECK("opendir", directory != NULL);
    int entries = 0;
    for (struct dirent *entry; (entry = readdir(directory)) != NULL;)
        ++entries;
    CHECK("readdir", closedir(directory) == 0 && entries == 2);

    CHECK("unlink", unlink("renamed.txt") == 0 && unlink("stream.txt") == 0);
    CHECK("chdir /", chdir("/") == 0);
    CHECK("rmdir", rmdir("/sdk-hello") == 0);

    /* Clocks. */
    struct timespec first, second;
    CHECK("clock_gettime", clock_gettime(CLOCK_MONOTONIC, &first) == 0);
    struct timespec pause = {0, 20 * 1000 * 1000};
    CHECK("nanosleep", nanosleep(&pause, NULL) == 0);
    CHECK("clock_gettime 2", clock_gettime(CLOCK_MONOTONIC, &second) == 0);
    long elapsed = (long)(second.tv_sec - first.tv_sec) * 1000 +
                   (second.tv_nsec - first.tv_nsec) / 1000000;
    CHECK("sleep length", elapsed >= 20);
    CHECK("realtime", clock_gettime(CLOCK_REALTIME, &second) == 0 && second.tv_sec > 0);

    /* Entropy is optional on a host: report instead of failing when it is absent. */
    unsigned char random[8] = {0};
    if (getentropy(random, sizeof(random)) == 0)
        printf("entropy: %02x%02x%02x%02x\n", random[0], random[1], random[2], random[3]);
    else
        printf("entropy: unavailable (errno %d)\n", errno);

    /* Unsupported calls say so. */
    int ends[2];
    CHECK("pipe", pipe(ends) == 0);
    CHECK("close pipe", close(ends[0]) == 0 && close(ends[1]) == 0);

    /* A stream the program never closes is flushed when it returns from main. */
    stream = fopen("/sdk-exit.txt", "w");
    CHECK("fopen unclosed", stream != NULL);
    fputs("flushed at exit\n", stream);

    puts("hello: ok");
    return argc > 1 ? atoi(argv[1]) : 0;
}
