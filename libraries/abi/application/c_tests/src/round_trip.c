#include "application.h"
#include <string.h>

int application_c_round_trip(const XilaDirectory *directory) {
    XilaFile *file = NULL;
    size_t count = 99;
    uint64_t position = 99;
    char buffer[5] = {0};
    if (xila_abi_version() != ((XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR)) return 1;
    if (xila_file_open(directory, "c-file", 6, XILA_OPEN_READ | XILA_OPEN_WRITE | XILA_OPEN_CREATE, &file) != XILA_STATUS_OK) return 2;
    if (xila_file_write(file, "hello", 5, &count) != XILA_STATUS_OK || count != 5) return 3;
    if (xila_file_seek(file, 0, XILA_SEEK_START, &position) != XILA_STATUS_OK || position != 0) return 4;
    if (xila_file_read(file, buffer, sizeof buffer, &count) != XILA_STATUS_OK || count != 5 || memcmp(buffer, "hello", 5) != 0) return 5;
    if (xila_file_flush(file) != XILA_STATUS_OK) return 6;
    return xila_file_close(file) == XILA_STATUS_OK ? 0 : 7;
}
