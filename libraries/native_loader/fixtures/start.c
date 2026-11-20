#include <xila/application.h>

/* ELF note descriptor is the packed version of the header used by the app. */
__attribute__((section(".note.xila.abi"), aligned(4), used))
static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} abi_note = {5, 4, 1, "XILA", (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR};

static const char answer[] = "ok";
#ifdef XILA_FIXTURE_NO_RELOCATIONS
#define expected answer
#else
static const char *volatile expected = answer; /* Force a RELATIVE relocation. */
#endif
static volatile unsigned int calls;           /* Exercise writable, zeroed BSS. */

#ifdef XILA_FIXTURE_CONSTRUCTOR
/* The loader runs no constructors, so it must reject the image. */
__attribute__((constructor)) static void construct(void) { calls = 0; }
#endif

int32_t _xila_start(const XilaStartup *startup) {
    if (startup->size != sizeof(*startup) || startup->argument_count > 1 ||
        !startup->api || !startup->standard_input || !startup->standard_output ||
        calls++ != 0)
        return 1;

    const XilaApi *api = startup->api;
    if (api->version != abi_note.version || api->size < sizeof(*api))
        return 3;

    uint64_t monotonic, resolution;
    int64_t realtime;
    if (api->time_monotonic(&monotonic) != XILA_STATUS_OK ||
        api->time_realtime(&realtime) != XILA_STATUS_OK || realtime <= 0)
        return 6;
    if (api->time_resolution(XILA_CLOCK_MONOTONIC, &resolution) != XILA_STATUS_OK ||
        resolution == 0 ||
        api->time_resolution(XILA_CLOCK_REALTIME, &resolution) != XILA_STATUS_OK ||
        resolution == 0)
        return 7;

    uint8_t random[8] = {0};
    XilaStatus status = api->random_fill(random, sizeof(random));
    if (startup->argument_count == 0)
        return status == XILA_STATUS_UNAVAILABLE ? 73 : 8;
    if (status != XILA_STATUS_OK)
        return 9;
    for (size_t index = 0; index < sizeof(random); ++index)
        if (random[index] != 0xa5)
            return 10;

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
