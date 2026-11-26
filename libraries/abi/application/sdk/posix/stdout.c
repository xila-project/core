/* stdout: descriptor 1 over the host-granted stream, in its own object so that an
 * image which never touches it carries neither the FILE nor its buffer. The buffer
 * size is a build-time constant (XILA_STDOUT_BUFFER). Line buffered. Defined
 * here rather than by picolibc's POSIX console, whose destructor the loader rejects;
 * exit() flushes it. */
#include <stdio.h>
#include <stdio-bufio.h>
#include <stdio-posix.h>

#ifndef XILA_STDOUT_BUFFER
#define XILA_STDOUT_BUFFER 128
#endif

static char stdout_buffer[XILA_STDOUT_BUFFER];

static struct __file_bufio stdout_file =
    FDEV_SETUP_POSIX(1, stdout_buffer, sizeof(stdout_buffer), __SWR, __BLBF);

FILE *const stdout = &stdout_file.xfile.cfile.file;

/* Back to the pristine initial state, for a relaunch of the same linked program after a
   previous run was cancelled: the FILE's lazily allocated lock and any bytes still sitting
   in its buffer lived on that run's heap, which the host freed with the abandoned
   coroutine. startup.c calls this hook through a weak no-op default, so an image that never touches stdout does
   not link it. */
__attribute__((visibility("hidden"))) void xila_stdout_reset(void) {
    stdout_file = (struct __file_bufio)FDEV_SETUP_POSIX(1, stdout_buffer, sizeof(stdout_buffer),
                                                        __SWR, __BLBF);
}
