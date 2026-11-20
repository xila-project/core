#ifndef XILA_POSIX_STDLIB_H
#define XILA_POSIX_STDLIB_H

#include_next <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Xila's getenv can adopt an assigned environ. A different symbol prevents
 * hosted compilers from treating the call as a read-only libc builtin. */
char *getenv(const char *name) __asm__("xila_getenv");

#ifdef __cplusplus
}
#endif

#endif
