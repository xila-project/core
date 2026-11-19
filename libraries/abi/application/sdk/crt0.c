#include "stubs.h"

/* One instance per loaded image. The host calls _xila_start once per image. */
extern const XilaApi xila_sdk_fallback_api;
const XilaApi *xila_sdk_api = &xila_sdk_fallback_api;

extern int32_t xila_main(const XilaStartup *startup);

int32_t _xila_start(const XilaStartup *startup) {
    if (startup == NULL || startup->size < sizeof(*startup) || startup->api == NULL ||
        startup->api->size < sizeof(XilaApi) ||
        startup->api->version != ((XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR)) {
        return -1;
    }

    /* Install the table before executing any application code, including init arrays. */
    xila_sdk_api = startup->api;
    int32_t result = xila_main(startup);
    xila_sdk_api = &xila_sdk_fallback_api;
    return result;
}
