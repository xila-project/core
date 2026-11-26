/* The POSIX C program of the real-SDK linked-archive end-to-end test
   (test_link_archive.py, RealSdkLinkedProgramTest): printf, getenv, malloc, atexit,
   getopt, strerror, arc4random, pthread_create and a blocking stdin read, compiled with
   -fPIE and localized with link-archive. */
#include <errno.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void handler(void) { fprintf(stderr, "atexit\n"); }

static void *unused_worker(void *arg) { return arg; }

int main(int argc, char **argv) {
    /* Fully buffered: nothing leaves stdout before exit or an explicit flush, so a
       cancelled run leaves its bytes in the buffer, where a relaunch must not find them. */
    setvbuf(stdout, NULL, _IOFBF, 512);
    fprintf(stderr, "R"); /* unbuffered ready signal */
    int saw_flag = 0;
    int option;
    while ((option = getopt(argc, argv, "a")) != -1) {
        if (option == 'a')
            saw_flag++;
    }
    /* Pull in thread.o, strerror.o and arc4random.o so every weak hook in POSIX and
       picolibc is tested against PIE linking. */
    pthread_t dummy_thread;
    if (argc < 0)
        (void)pthread_create(&dummy_thread, NULL, unused_worker, NULL);
    uint32_t sample = arc4random();
    const char *enoent = strerror(ENOENT);
    char *value = getenv("XILA_E2E");
    printf("hello %s flag=%d err=%d rnd=%d\n", value != NULL ? value : "<null>", saw_flag,
           enoent != NULL && enoent[0] != '\0', (sample | 1u) != 0);
    void *p = malloc(100);
    memset(p, 0x41, 100);
    free(p);
    atexit(handler);
    printf("partial");
    char line[64];
    if (fgets(line, sizeof line, stdin) != NULL)
        printf("read:%s", line);
    return 0;
}
