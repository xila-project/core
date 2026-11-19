#include <xila/application.h>

/* ELF note descriptor is the packed version of the header used by the app. */
__attribute__((section(".note.xila.abi"), aligned(4), used))
static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} abi_note = {5, 4, 1, "XILA", (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR};

static const char answer[] = "ok";
static const char *volatile expected = answer; /* Force a RELATIVE relocation. */
static volatile unsigned int calls;           /* Exercise writable, zeroed BSS. */

int32_t _xila_start(const XilaStartup *startup) {
    if (startup->size != sizeof(*startup) || startup->argument_count != 0 ||
        !startup->api || !startup->standard_input || !startup->standard_output ||
        calls++ != 0)
        return 1;

    const XilaApi *api = startup->api;
    if (api->version != abi_note.version || api->size < sizeof(*api))
        return 3;

    char bytes[2];
    size_t count = 0;
    if (api->file_read(startup->standard_input, bytes, sizeof(bytes), &count) != XILA_STATUS_OK ||
        count != sizeof(bytes) || bytes[0] != expected[0] || bytes[1] != expected[1])
        return 4;
    if (api->file_write(startup->standard_output, bytes, count, &count) != XILA_STATUS_OK ||
        count != sizeof(bytes))
        return 5;
    return 73;
}
