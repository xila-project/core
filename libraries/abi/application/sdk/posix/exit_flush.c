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
 * the layer's single-threaded exit path calls fflush(NULL). */
#include <stdio.h>
#include <stdio-bufio.h>

#include "internal.h"

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

/* Match picolibc's weak references so images using neither stream still
 * omit their FILE objects and buffers. */
extern FILE *const stdout __attribute__((weak));
extern FILE *const stderr __attribute__((weak));

/* Only a descriptor-backed buffered stream names a descriptor: one whose functions take
   a pointer (__BFPTR) holds a pointer in the same field. */
static void keep_stream_descriptor(FILE *file) {
    if (!(file->flags & __SBUF))
        return;
    struct __file_bufio *buffered = (struct __file_bufio *)file;
    if (!(buffered->bflags & __BFPTR))
        xila_fd_keep_for_exit_flush((int)(intptr_t)buffered->ptr);
}

static void flush_available(FILE *file) {
    if (file->lock == NULL) {
        /* No thread owns this stream. Avoid lazy lock initialization during
           shutdown, which would need the global libc lock. */
        if (file->flush != NULL)
            (void)file->flush(file);
    } else if (ftrylockfile(file) == 0) {
        (void)fflush(file);
        funlockfile(file);
    }
}

void xila_stdio_exit_with_stopped_threads(void) {
    for (FILE *file = __stdio_file_list; file != NULL; file = ((struct __file_bufio *)file)->next)
        keep_stream_descriptor(file);
    if (&stdout)
        keep_stream_descriptor(stdout);
    if (&stderr)
        keep_stream_descriptor(stderr);

    /* Close raw descriptors first, so pipe readers can see EOF. Keep the
       descriptors backing FILE buffers open until their final writes. */
    xila_fd_close_before_exit_flush();
    for (FILE *file = __stdio_file_list; file != NULL; file = ((struct __file_bufio *)file)->next)
        flush_available(file);
    if (&stdout)
        flush_available(stdout);
    if (&stderr)
        flush_available(stderr);
}
