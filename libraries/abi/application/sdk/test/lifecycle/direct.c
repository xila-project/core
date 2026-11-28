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

static void *worker(void *argument) {
    return argument;
}

int32_t xila_main(const XilaStartup *startup) {
    started = startup;
    if (initialized != 1)
        return 1;
    if (startup->argument_count != 0) {
        size_t thread;
        bool live = false;
        if (startup->api->thread_spawn(startup->process, 0, worker, NULL, &thread) != XILA_STATUS_OK ||
            startup->api->thread_has_workers(&live) != XILA_STATUS_OK || !live)
            return 2;
    }
    return 23;
}
