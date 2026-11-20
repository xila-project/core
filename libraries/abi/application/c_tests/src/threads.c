#include "application.h"

#define CHECK(condition) do { if (!(condition)) return __LINE__; } while (0)
#define OK(call) CHECK((call) == XILA_STATUS_OK)

struct Shared {
    const XilaApi *api;
    const XilaProcess *process;
    XilaMutex mutex;
    XilaCondition condition;
    size_t key;
    size_t main_id;
    size_t peer_id;
    bool ready;
};

static void *worker(void *argument) {
    struct Shared *shared = argument;
    const XilaApi *api = shared->api;
    size_t id = 0;
    void *value = shared;
    if (api->thread_current(&id) != XILA_STATUS_OK || id == shared->main_id ||
        api->thread_join(shared->main_id, NULL) != XILA_STATUS_NO_SUCH_TASK ||
        api->thread_join(shared->peer_id, NULL) != XILA_STATUS_NO_CHILD ||
        api->thread_tls_get(shared->key, &value) != XILA_STATUS_OK || value != NULL ||
        api->thread_tls_set(shared->key, shared) != XILA_STATUS_OK ||
        api->mutex_lock(&shared->mutex, 0) != XILA_STATUS_OK)
        (void)api->process_exit(shared->process, __LINE__);

    shared->ready = true;
    if (api->condition_notify(&shared->condition, false) != XILA_STATUS_OK ||
        api->mutex_unlock(&shared->mutex) != XILA_STATUS_OK)
        return NULL;
    return shared;
}

static void *peer(void *argument) { return argument; }

int32_t threads_main(const XilaStartup *startup) {
    const XilaApi *api = startup->api;
    struct Shared shared = { .api = api, .process = startup->process };
    const XilaProcess *process = startup->process;
    size_t id = 0;
    OK(api->thread_current(&shared.main_id));
    CHECK(api->thread_spawn(process, 1, worker, &shared, &id) == XILA_STATUS_INVALID_ARGUMENT);
    CHECK(id == 0);
    CHECK(api->thread_spawn(process, 0, NULL, &shared, &id) == XILA_STATUS_INVALID_ARGUMENT);

    OK(api->thread_key_create(&shared.key));
    OK(api->thread_tls_set(shared.key, &shared.mutex));
    OK(api->mutex_init(&shared.mutex, XILA_MUTEX_RECURSIVE));
    OK(api->mutex_lock(&shared.mutex, 0));
    OK(api->mutex_lock(&shared.mutex, 0));
    OK(api->mutex_unlock(&shared.mutex));

    OK(api->thread_spawn(process, 32768, peer, &shared, &shared.peer_id));
    OK(api->thread_spawn(process, 32768, worker, &shared, &id));
    bool live = false;
    OK(api->thread_has_workers(&live));
    CHECK(live);
    while (!shared.ready)
        OK(api->condition_wait(&shared.condition, &shared.mutex, 0, NULL));
    OK(api->mutex_unlock(&shared.mutex));

    void *value = NULL;
    OK(api->thread_join(id, &value));
    CHECK(value == &shared);
    OK(api->thread_has_workers(&live));
    CHECK(!live); /* The finished peer is still joinable. */
    OK(api->thread_join(shared.peer_id, NULL));
    CHECK(api->thread_join(id, NULL) == XILA_STATUS_NO_SUCH_TASK);
    OK(api->thread_tls_get(shared.key, &value));
    CHECK(value == &shared.mutex);
    size_t stale_key = shared.key;
    OK(api->thread_key_delete(stale_key));
    OK(api->thread_key_create(&shared.key));
    CHECK(api->thread_tls_get(stale_key, &value) == XILA_STATUS_INVALID_ARGUMENT);
    OK(api->thread_tls_get(shared.key, &value));
    CHECK(value == NULL);
    OK(api->thread_key_delete(shared.key));
    OK(api->condition_destroy(&shared.condition));
    OK(api->mutex_destroy(&shared.mutex));
    return 0;
}
