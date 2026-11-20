#include <unistd.h>

#include "internal.h"

pid_t getpid(void) {
    uint32_t id;
    XilaStatus status = xila_process_id(xila_process.process, &id);
    return status == XILA_STATUS_OK ? (pid_t)id : xila_fail(status);
}

pid_t getppid(void) {
    uint32_t id;
    XilaStatus status = xila_process_parent_id(xila_process.process, &id);
    return status == XILA_STATUS_OK ? (pid_t)id : xila_fail(status);
}
