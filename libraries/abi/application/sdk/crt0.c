#include "stubs.h"

/* The loader requires this PT_NOTE record: name "XILA", type 1, and the packed ABI
   version of the header the image was built against. */
__attribute__((section(".note.xila.abi"), aligned(4), used)) static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} xila_abi_note = {5, 4, 1, "XILA", (XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR};

/* One instance per loaded image. The host calls _xila_start once per image. */
extern const XilaApi xila_sdk_fallback_api;
const XilaApi *xila_sdk_api = &xila_sdk_fallback_api;
const XilaProcess *xila_sdk_process = NULL;

extern int32_t xila_main(const XilaStartup *startup);

int32_t _xila_start(const XilaStartup *startup) {
    if (startup == NULL || startup->size < sizeof(*startup) || startup->api == NULL ||
        startup->api->size < sizeof(XilaApi) ||
        startup->api->version != ((XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR)) {
        return -1;
    }

    /* Install the table before calling the application. Code that runs before _xila_start sees
       the fallback table. */
    xila_sdk_api = startup->api;
    xila_sdk_process = startup->process;
    int32_t result = xila_main(startup);
    xila_sdk_process = NULL;
    xila_sdk_api = &xila_sdk_fallback_api;
    return result;
}
