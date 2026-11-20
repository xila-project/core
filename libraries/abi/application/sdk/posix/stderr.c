/* stderr: descriptor 2 over the host-granted stream, in its own object so that an image
 * which never touches it carries neither the FILE nor its buffer. Unbuffered, as C and
 * POSIX specify: picolibc's bufio treats a one-byte buffer that is not line buffered as
 * unbuffered (the same state setvbuf(_IONBF) produces), so every write reaches the host
 * when it is made, which is what a diagnostic needs when the program dies next.
 * Defined here rather than by picolibc's POSIX console, whose destructor the loader
 * rejects. */
#include <stdio.h>
#include <stdio-bufio.h>
#include <stdio-posix.h>

static char stderr_buffer[1];

static struct __file_bufio stderr_file =
    FDEV_SETUP_POSIX(2, stderr_buffer, sizeof(stderr_buffer), __SWR, 0);

FILE *const stderr = &stderr_file.xfile.cfile.file;
