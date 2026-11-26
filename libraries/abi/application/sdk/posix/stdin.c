/* stdin: descriptor 0 over the host-granted stream, in its own object so that an
 * image which never touches it carries neither the FILE nor its buffer. The buffer
 * size is a build-time constant (XILA_STDIN_BUFFER). Defined
 * here rather than by picolibc's POSIX console, whose destructor the loader rejects;
 * stdin has nothing to flush. */
#include <stdio.h>
#include <stdio-bufio.h>
#include <stdio-posix.h>

#ifndef XILA_STDIN_BUFFER
#define XILA_STDIN_BUFFER 64
#endif

static char stdin_buffer[XILA_STDIN_BUFFER];

static struct __file_bufio stdin_file =
    FDEV_SETUP_POSIX(0, stdin_buffer, sizeof(stdin_buffer), __SRD, 0);

FILE *const stdin = &stdin_file.xfile.cfile.file;

/* Back to the pristine initial state, for a relaunch of the same linked program after a
   previous run was cancelled: the FILE's lazily allocated lock lived on that run's heap,
   which the host freed with the abandoned coroutine. startup.c calls this hook through a weak no-op default, so
   an image that never touches stdin does not link it. */
__attribute__((visibility("hidden"))) void xila_stdin_reset(void) {
    stdin_file = (struct __file_bufio)FDEV_SETUP_POSIX(0, stdin_buffer, sizeof(stdin_buffer),
                                                       __SRD, 0);
}
