/* The exit-time stdio flush picolibc would provide, without its constructor and its
 * forgetting.
 *
 * picolibc keeps every FILE opened through fopen in __stdio_file_list so that exit()
 * can flush the ones the program never closed. Its own _bufio_exit_flush is built into
 * an archive member that also registers itself with atexit() from a constructor, which
 * the loader rejects, and it unlinks each stream from the list as it flushes it, so a
 * program that called fflush(NULL) lost every later write at exit. tools/sdk/build.py
 * removes that member from libc.a; this one replaces it. The list is walked, never
 * edited: fclose() unlinks a stream, a flush does not.
 *
 * fflush(NULL) calls _bufio_exit_flush and then flushes stdout and stderr itself, and
 * the layer's exit() calls fflush(NULL). _exit() calls neither. */
#include <stdio.h>
#include <stdio-bufio.h>

#ifndef __STDIO_EXIT_FLUSH
#error "picolibc must be built with stdio-exit-flush"
#endif

FILE *__stdio_file_list;

void _bufio_exit_flush(void) {
    for (FILE *file = __stdio_file_list; file != NULL;) {
        FILE *next = ((struct __file_bufio *)file)->next;
        (void)fflush(file);
        file = next;
    }
}
