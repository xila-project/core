#include <xila/application.h>

static const XilaStartup *started;
static int initialized;

__attribute__((constructor))
static void initialize(void) {
    initialized = 1;
}

__attribute__((destructor))
static void finalize(void) {
    if (started != NULL) {
        static const char message[] = "direct fini\n";
        size_t written;
        started->api->file_write(started->standard_output, message, sizeof(message) - 1, &written);
    }
}

int32_t xila_main(const XilaStartup *startup) {
    started = startup;
    return initialized == 1 ? 23 : 1;
}
