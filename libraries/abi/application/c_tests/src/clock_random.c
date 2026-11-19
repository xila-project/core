#include "application.h"
#include <string.h>

#define CHECK(expression) do { if (!(expression)) return __LINE__; } while (0)

/*
 * Exercises the clock resolution, sleep and random calls through the startup
 * table. The first argument is "synchronized" or "unsynchronized" and states
 * the expected realtime clock. The host's entropy source is a test device that
 * returns at most three bytes per read, is pending on every other read, fills
 * with 0xA5, fails with Unavailable for 13-byte requests and makes no progress
 * for 17-byte requests.
 */
int32_t startup_clock_random_main(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    CHECK(startup->argument_count == 1);
    const int synchronized = startup->arguments[0].data[0] == 's';
    uint64_t monotonic_resolution = 0;
    uint64_t realtime_resolution = 0;
    uint64_t marker = 7;
    int64_t realtime = 0;
    CHECK(api->time_resolution(XILA_CLOCK_MONOTONIC, &monotonic_resolution) == XILA_STATUS_OK);
    CHECK(monotonic_resolution > 0);
    CHECK(api->time_resolution(XILA_CLOCK_REALTIME, &realtime_resolution) == XILA_STATUS_OK);
    if (synchronized) {
        CHECK(api->time_realtime(&realtime) == XILA_STATUS_OK);
        CHECK(realtime_resolution >= monotonic_resolution);
    } else {
        /* The resolution stays readable while UTC is invalid. */
        CHECK(api->time_realtime(&realtime) == XILA_STATUS_UNAVAILABLE);
        CHECK(realtime_resolution == monotonic_resolution);
    }
    CHECK(api->time_resolution(2, &marker) == XILA_STATUS_INVALID_ARGUMENT && marker == 7);
    CHECK(api->time_resolution(XILA_CLOCK_MONOTONIC, NULL) == XILA_STATUS_INVALID_ARGUMENT);

    uint64_t before = 0;
    uint64_t after = 0;
    CHECK(api->time_sleep(0) == XILA_STATUS_OK);
    CHECK(api->time_monotonic(&before) == XILA_STATUS_OK);
    CHECK(api->time_sleep(2000000) == XILA_STATUS_OK);
    CHECK(api->time_monotonic(&after) == XILA_STATUS_OK);
    CHECK(after - before >= 2000000);

    unsigned char buffer[100];
    memset(buffer, 0, sizeof buffer);
    CHECK(api->random_fill(NULL, 0) == XILA_STATUS_OK);
    CHECK(api->random_fill(buffer, 0) == XILA_STATUS_OK && buffer[0] == 0);
    CHECK(api->random_fill(NULL, 1) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->random_fill(buffer, SIZE_MAX) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(api->random_fill(buffer, 1) == XILA_STATUS_OK);
    CHECK(buffer[0] == 0xA5 && buffer[1] == 0);
    /* 99 bytes take 33 short reads, none of which has the failing sizes. */
    CHECK(api->random_fill(buffer, 99) == XILA_STATUS_OK);
    for (size_t i = 0; i < 99; i++) CHECK(buffer[i] == 0xA5);
    CHECK(buffer[99] == 0);
    CHECK(api->random_fill(buffer, 13) == XILA_STATUS_UNAVAILABLE);
    CHECK(api->random_fill(buffer, 17) == XILA_STATUS_INPUT_OUTPUT);
    return 0;
}
