#include "stubs.h"
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
