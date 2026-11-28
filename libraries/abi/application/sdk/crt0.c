#include "stubs.h"
#include "version.h"

#ifndef XILA_SDK_LINKED
/* The loader requires this PT_NOTE record: name "XILA", type 1, and the Xila version of the
   header the image was built against, packed as (major << 16) | minor. */
__attribute__((section(".note.xila.abi"), aligned(4), used)) static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
    uint32_t version;
} xila_abi_note = {5, 4, 1, "XILA", (XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR};
/* The entry drives validated lifecycle arrays. */
__attribute__((section(".note.xila.abi.lifecycle"), aligned(4), used)) static const struct {
    uint32_t name_size, descriptor_size, type;
    char name[8];
} xila_lifecycle_note = {5, 0, 2, "XILA"};
#endif

/* One instance per loaded image. The host calls _xila_start once per image; a linked
   program's host calls xila_linked_start once per running instance. The firmware's link
   localizes every other symbol of a linked program (llvm-objcopy), so these globals are
   private to it. */
extern const XilaApi xila_sdk_fallback_api;
const XilaApi *xila_sdk_api = &xila_sdk_fallback_api;
const XilaProcess *xila_sdk_process = NULL;

extern int32_t xila_main(const XilaStartup *startup);

#ifndef XILA_SDK_LINKED

/* C++ objects use this identifier when they register an exit handler. */
__attribute__((weak, visibility("hidden"))) void *__dso_handle = &__dso_handle;

/* The POSIX startup overrides this when it owns the lifecycle. */
__attribute__((weak, visibility("hidden"), noinline)) bool xila_posix_owns_lifecycle(void) {
    return false;
}

int32_t _xila_start(const XilaStartup *startup) {
    if (startup == NULL || startup->size < offsetof(XilaStartup, lifecycle) || startup->api == NULL ||
        startup->api->size < sizeof(XilaApi) ||
        !xila_version_compatible(xila_abi_note.version, startup->api->version)) {
        return -1;
    }

    /* Install the table before calling the application. Code that runs before _xila_start sees
       the fallback table. */
    xila_sdk_api = startup->api;
    xila_sdk_process = startup->process;
    const XilaLifecycle *lifecycle = startup->size >= sizeof(*startup) ? startup->lifecycle : NULL;
    if (lifecycle != NULL && xila_posix_owns_lifecycle())
        lifecycle = NULL;
    if (lifecycle != NULL)
        lifecycle->run(lifecycle, false);
    int32_t result = xila_main(startup);
    if (lifecycle != NULL)
        lifecycle->run(lifecycle, true);
    xila_sdk_process = NULL;
    xila_sdk_api = &xila_sdk_fallback_api;
    return result;
}

#else /* XILA_SDK_LINKED */

/* A program linked into the firmware has no image for a loader to check: no ABI note and
   no _xila_start. The host calls xila_linked_start once per running instance, with the
   same startup and version compatibility check as _xila_start. */
int32_t xila_linked_start(const XilaStartup *startup) {
    /* A previous run of this program may have been cancelled while suspended: it ran no
       cleanup, so the table pointers can still name that run's startup block. Reset them
       before anything can read them. */
    xila_sdk_api = &xila_sdk_fallback_api;
    xila_sdk_process = NULL;
    if (startup == NULL || startup->size < offsetof(XilaStartup, lifecycle) || startup->api == NULL ||
        startup->api->size < sizeof(XilaApi) ||
        !xila_version_compatible((XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR,
                                 startup->api->version)) {
        return -1;
    }

    /* Install the table before calling the application. Code that runs before
       xila_linked_start sees the fallback table. */
    xila_sdk_api = startup->api;
    xila_sdk_process = startup->process;
    int32_t result = xila_main(startup);
    xila_sdk_process = NULL;
    xila_sdk_api = &xila_sdk_fallback_api;
    return result;
}

#endif /* XILA_SDK_LINKED */
