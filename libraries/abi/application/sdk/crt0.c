#include "stubs.h"

/* The loader requires this PT_NOTE record: name "XILA", type 1, and the Xila version of the
   header the image was built against, packed as (major << 16) | minor. */
__attribute__((section(".note.xila.abi"), aligned(4), used)) static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} xila_abi_note = {5, 4, 1, "XILA", (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR};

/* One instance per loaded image. The host calls _xila_start once per image. */
extern const XilaApi xila_sdk_fallback_api;
const XilaApi *xila_sdk_api = &xila_sdk_fallback_api;
const XilaProcess *xila_sdk_process = NULL;

extern int32_t xila_main(const XilaStartup *startup);

/* Whether a host running the packed Xila version `running` can run an image built for the
   packed version `declared`: before 1.0 the minor versions must be equal; from 1.0 the majors
   must be equal and the running minor at least the declared one. This mirrors
   xila_version::compatible in Core and xila_package_manifest::compatible in package_manager. */
static bool xila_version_compatible(uint32_t declared, uint32_t running) {
    uint32_t major = declared >> 16, minor = declared & 0xffff;
    if (major != running >> 16) return false;
    return major == 0 ? minor == (running & 0xffff) : minor <= (running & 0xffff);
}

int32_t _xila_start(const XilaStartup *startup) {
    if (startup == NULL || startup->size < sizeof(*startup) || startup->api == NULL ||
        startup->api->size < sizeof(XilaApi) ||
        !xila_version_compatible(xila_abi_note.version, startup->api->version)) {
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
