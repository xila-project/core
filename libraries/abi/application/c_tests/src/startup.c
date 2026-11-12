#include "application.h"
#include <assert.h>
#include <string.h>

int32_t startup_test_main(const XilaStartup *startup) {
    assert(startup != NULL && startup->size == sizeof(*startup));
    assert(startup->argument_count == 2);
    assert(startup->arguments[0].length == 0);
    assert(startup->arguments[1].length == 6);
    assert(memcmp(startup->arguments[1].data, "h\xc3\xa9llo", 6) == 0);
    assert(startup->standard_input == NULL && startup->standard_error == NULL);
    size_t count = 0;
    assert(xila_file_write(startup->standard_output, "startup", 7, &count) == XILA_STATUS_OK);
    assert(count == 7);
    XilaFile *file = NULL;
    assert(xila_file_open(startup->home, "startup-home", 12,
        XILA_OPEN_WRITE | XILA_OPEN_CREATE, &file) == XILA_STATUS_OK);
    assert(xila_file_close(file) == XILA_STATUS_OK);
    return 23;
}

int32_t startup_empty_main(const XilaStartup *startup) {
    assert(startup->argument_count == 0 && startup->arguments == NULL);
    assert(startup->standard_input == NULL && startup->standard_output == NULL);
    assert(startup->standard_error == NULL && startup->home == NULL);
    return -7;
}
