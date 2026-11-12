#include "application.h"
#include <string.h>

int application_c_round_trip(const XilaDirectory *directory) {
    XilaFile *file = NULL;
    size_t count = 99;
    uint64_t position = 99;
    char buffer[5] = {0};
    if (xila_abi_version() != ((XILA_ABI_VERSION_MAJOR << 16) | XILA_ABI_VERSION_MINOR)) return 1;
    if (xila_file_open(directory, "c-file", 6, XILA_OPEN_READ | XILA_OPEN_WRITE | XILA_OPEN_CREATE, &file) != XILA_STATUS_OK) return 2;
    if (xila_file_write(file, "hello", 5, &count) != XILA_STATUS_OK || count != 5) return 3;
    if (xila_file_seek(file, 0, XILA_SEEK_START, &position) != XILA_STATUS_OK || position != 0) return 4;
    if (xila_file_read(file, buffer, sizeof buffer, &count) != XILA_STATUS_OK || count != 5 || memcmp(buffer, "hello", 5) != 0) return 5;
    if (xila_file_flush(file) != XILA_STATUS_OK) return 6;
    return xila_file_close(file) == XILA_STATUS_OK ? 0 : 7;
}

#include <time.h>
#define CHECK(call) do { if ((call) != XILA_STATUS_OK) return __LINE__; } while (0)
#define REQUIRE(test) do { if (!(test)) return __LINE__; } while (0)

struct shared {
    XilaMutex *mutex;
    XilaConditionVariable *condition;
    int counter;
    int ready;
};

static int32_t increment(void *argument) {
    struct shared *shared = argument;
    for (int i = 0; i < 1000; ++i) {
        CHECK(xila_mutex_lock(shared->mutex));
        ++shared->counter;
        CHECK(xila_mutex_unlock(shared->mutex));
    }
    return 42;
}
static int32_t probe(void *argument) {
    struct shared *shared = argument;
    return (int32_t)xila_mutex_try_lock(shared->mutex);
}
static int32_t produce(void *argument) {
    struct shared *shared = argument;
    CHECK(xila_mutex_lock(shared->mutex));
    shared->ready = 1;
    CHECK(xila_condvar_signal(shared->condition));
    CHECK(xila_mutex_unlock(shared->mutex));
    return 19;
}
static int32_t consume(void *argument) {
    struct shared *shared = argument;
    CHECK(xila_mutex_lock(shared->mutex));
    ++shared->counter;
    CHECK(xila_condvar_broadcast(shared->condition));
    while (!shared->ready) CHECK(xila_condvar_wait(shared->condition, shared->mutex));
    CHECK(xila_mutex_unlock(shared->mutex));
    return 23;
}
static uint64_t now(void) {
    struct timespec value;
    if (timespec_get(&value, TIME_UTC) != TIME_UTC) return 0;
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}
int application_c_threads(void) {
    struct shared shared = {0};
    XilaThread *threads[4] = {0};
    int32_t result = -1;
    CHECK(xila_mutex_create(0, &shared.mutex));
    CHECK(xila_condvar_create(&shared.condition));
    for (int i = 0; i < 4; ++i) CHECK(xila_thread_spawn(increment, &shared, NULL, &threads[i]));
    for (int i = 0; i < 4; ++i) {
        CHECK(xila_thread_join(threads[i], &result));
        REQUIRE(result == 42);
    }
    REQUIRE(shared.counter == 4000);
    CHECK(xila_mutex_lock(shared.mutex));
    CHECK(xila_thread_spawn(probe, &shared, NULL, &threads[0]));
    CHECK(xila_thread_join(threads[0], &result));
    REQUIRE(result == XILA_STATUS_WOULD_BLOCK);
    REQUIRE(xila_mutex_destroy(shared.mutex) == XILA_STATUS_BUSY);
    CHECK(xila_thread_spawn(produce, &shared, NULL, &threads[0]));
    while (!shared.ready) CHECK(xila_condvar_wait(shared.condition, shared.mutex));
    CHECK(xila_mutex_unlock(shared.mutex));
    CHECK(xila_thread_join(threads[0], &result));
    REQUIRE(result == 19);
    CHECK(xila_mutex_lock(shared.mutex));
    uint64_t start = now();
    REQUIRE(xila_condvar_wait_timeout(shared.condition, shared.mutex, 2000000) == XILA_STATUS_TIMED_OUT);
    REQUIRE(now() - start >= 2000000);
    CHECK(xila_mutex_unlock(shared.mutex));
    start = now();
    CHECK(xila_thread_sleep(2000000));
    REQUIRE(now() - start >= 2000000);
    CHECK(xila_thread_yield());
    CHECK(xila_mutex_lock(shared.mutex));
    shared.ready = 0;
    shared.counter = 0;
    for (int i = 0; i < 3; ++i) CHECK(xila_thread_spawn(consume, &shared, NULL, &threads[i]));
    while (shared.counter != 3) CHECK(xila_condvar_wait(shared.condition, shared.mutex));
    shared.ready = 1;
    CHECK(xila_condvar_broadcast(shared.condition));
    CHECK(xila_mutex_unlock(shared.mutex));
    for (int i = 0; i < 3; ++i) {
        CHECK(xila_thread_join(threads[i], &result));
        REQUIRE(result == 23);
    }
    shared.ready = 0;
    CHECK(xila_mutex_lock(shared.mutex));
    CHECK(xila_thread_spawn(produce, &shared, NULL, &threads[0]));
    CHECK(xila_thread_detach(threads[0]));
    while (!shared.ready) CHECK(xila_condvar_wait(shared.condition, shared.mutex));
    /* Producer has released the mutex and no longer accesses shared storage. */
    CHECK(xila_mutex_unlock(shared.mutex));
    CHECK(xila_condvar_broadcast(shared.condition));
    CHECK(xila_condvar_destroy(shared.condition));
    CHECK(xila_mutex_destroy(shared.mutex));
    return 0;
}
