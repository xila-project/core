/* getrandom for Xila native applications: bytes from the host-granted entropy source. */
#ifndef _XILA_SYS_RANDOM_H
#define _XILA_SYS_RANDOM_H

#include <stddef.h>
#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

#define GRND_NONBLOCK 1
#define GRND_RANDOM 2

ssize_t getrandom(void *buffer, size_t size, unsigned int flags);

#ifdef __cplusplus
}
#endif

#endif
