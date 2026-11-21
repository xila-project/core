#include <xila/application.h>

/* ELF note descriptor is the packed version of the header used by the app. */
__attribute__((section(".note.xila.abi"), aligned(4), used))
static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} abi_note = {5, 4, 1, "XILA", (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR};

/* Blocks in a read of the standard input, which the test never answers, so that the host
   has to cancel the program while it is suspended. */
int32_t _xila_start(const XilaStartup *startup) {
    char byte;
    size_t count = 0;
    startup->api->file_read(startup->standard_input, &byte, sizeof(byte), &count);
    return 7;
}
