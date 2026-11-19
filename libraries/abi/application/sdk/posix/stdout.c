/* stdout: descriptor 1 over the host-granted stream, in its own object so that an
 * image which never touches it carries neither the FILE nor its buffer. The buffer
 * size is a build-time constant (XILA_STDOUT_BUFFER). Line buffered. Defined
 * here rather than by picolibc's POSIX console, whose destructor the loader rejects;
 * xila_posix_shutdown flushes it. */
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
