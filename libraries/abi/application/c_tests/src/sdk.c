#include "stubs.h"
#include "version.h"
#include <assert.h>
#include <string.h>

/* The SDK entry is linked separately from the direct-table client. */
int32_t xila_main(const XilaStartup *startup) {
    void *allocation = NULL;
    assert(xila_heap_allocate(startup->heap, 5, &allocation) == XILA_STATUS_OK);
    memcpy(allocation, "xila", 5);
    assert(xila_heap_reallocate(startup->heap, allocation, 10, &allocation) == XILA_STATUS_OK);
    assert(memcmp(allocation, "xila", 5) == 0);
    assert(xila_heap_free(startup->heap, allocation) == XILA_STATUS_OK);
    return 42;
}

int32_t sdk_test_startup(const XilaStartup *startup) {
    assert(xila_file_flush(NULL) == XILA_STATUS_UNAVAILABLE);
    assert(_xila_start(NULL) == -1);
    assert(xila_file_flush(NULL) == XILA_STATUS_UNAVAILABLE);
    int32_t result = _xila_start(startup);
    assert(xila_file_flush(NULL) == XILA_STATUS_UNAVAILABLE);
    return result;
}

void sdk_test_versions(void) {
    /* The same boundary cases as xila_version and xila_package_manifest. */
    assert(xila_version_compatible(0x00000001, 0x00000001));
    assert(!xila_version_compatible(0x00000001, 0x00000002));
    assert(!xila_version_compatible(0x00000002, 0x00000001));
    assert(!xila_version_compatible(0x00010003, 0x00010002));
    assert(xila_version_compatible(0x00010003, 0x00010003));
    assert(xila_version_compatible(0x00010003, 0x00010009));
    assert(!xila_version_compatible(0x00010003, 0x00020000));
    assert(!xila_version_compatible(0x00000009, 0x00010000));
    assert(xila_version_compatible(0, 0));
    assert(!xila_version_compatible(0, 1));
    assert(!xila_version_compatible(1, 0));
}
