/* stderr: descriptor 2 over the host-granted stream, in its own object so that an
 * image which never touches it carries neither the FILE nor its buffer. The buffer
 * size is a build-time constant (XILA_STDERR_BUFFER). Line buffered. Defined
 * here rather than by picolibc's POSIX console, whose destructor the loader rejects;
 * xila_posix_shutdown flushes it. */
#include <stdio.h>
#include <stdio-bufio.h>
#include <stdio-posix.h>

#ifndef XILA_STDERR_BUFFER
#define XILA_STDERR_BUFFER 64
#endif

static char stderr_buffer[XILA_STDERR_BUFFER];

static struct __file_bufio stderr_file =
    FDEV_SETUP_POSIX(2, stderr_buffer, sizeof(stderr_buffer), __SWR, __BLBF);

FILE *const stderr = &stderr_file.xfile.cfile.file;
