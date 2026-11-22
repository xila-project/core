/* ENOSYS stubs for POSIX calls whose implementations are pending. */
#include <sys/types.h>
#include <unistd.h>

#include "internal.h"

pid_t fork(void) { return xila_fail_errno(ENOSYS); } /* never provided: no MMU */
