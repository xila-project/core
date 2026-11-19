#include "application.h"
#include <assert.h>
#include <string.h>

int32_t startup_test_main(const XilaStartup *startup) {
    assert(startup != NULL && startup->size == sizeof(*startup));
    const XilaApi *api = startup->api;
    assert(api->size == sizeof(*api));
    uint64_t monotonic = 0;
    int64_t realtime = 0;
    assert(api->time_monotonic(&monotonic) == XILA_STATUS_OK);
    assert(api->time_realtime(&realtime) == XILA_STATUS_OK);
    assert(startup->argument_count == 2);
    assert(startup->arguments[0].length == 0);
    assert(startup->arguments[1].length == 6);
    assert(memcmp(startup->arguments[1].data, "h\xc3\xa9llo", 6) == 0);
    assert(startup->standard_input == NULL && startup->standard_error == NULL);
    size_t count = 0;
    assert(api->file_write(startup->standard_output, "startup", 7, &count) == XILA_STATUS_OK);
    assert(count == 7);
    XilaFile *file = NULL;
    assert(api->file_open(startup->home, "startup-home", 12,
        XILA_OPEN_WRITE | XILA_OPEN_CREATE, &file) == XILA_STATUS_OK);
    assert(api->file_close(file) == XILA_STATUS_OK);
    return 23;
}

int32_t startup_empty_main(const XilaStartup *startup) {
    assert(startup->argument_count == 0 && startup->arguments == NULL);
    assert(startup->standard_input == NULL && startup->standard_output == NULL);
    assert(startup->standard_error == NULL && startup->home == NULL);
    return -7;
}

int32_t startup_heap_main(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    assert(api->size == sizeof(*api));
    assert(api->version == ((XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR));
    void *first = NULL;
    void *last = NULL;
    assert(api->heap_allocate(startup->heap, 24, &first) == XILA_STATUS_OK);
    assert(api->heap_allocate(startup->heap, 0, &last) == XILA_STATUS_OK);
    assert(first != NULL && last != NULL && first != last);
    assert((uintptr_t)first % XILA_HEAP_ALIGNMENT == 0);
    memset(first, 0x63, 24);
    assert(api->heap_reallocate(startup->heap, first, SIZE_MAX, &first) == XILA_STATUS_OUT_OF_MEMORY);
    assert(api->heap_reallocate(startup->heap, first, 96, &first) == XILA_STATUS_OK);
    assert((uintptr_t)first % XILA_HEAP_ALIGNMENT == 0);
    for (size_t i = 0; i < 24; i++) assert(((unsigned char *)first)[i] == 0x63);
    assert(api->heap_reallocate(startup->heap, first, 4, &first) == XILA_STATUS_OK);
    for (size_t i = 0; i < 4; ++i) assert(((unsigned char *)first)[i] == 0x63);
    assert(api->heap_reallocate(startup->heap, last, 0, &last) == XILA_STATUS_OK);
    assert(last == NULL);
    assert(api->heap_reallocate(startup->heap, NULL, 7, &last) == XILA_STATUS_OK);
    assert(api->heap_free(startup->heap, last) == XILA_STATUS_OK);
    assert(api->heap_free(startup->heap, NULL) == XILA_STATUS_OK);
    /* Leave the resized allocation for the process owner. */
    return 0;
}
