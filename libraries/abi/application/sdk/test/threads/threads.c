#include <errno.h>
#include <pthread.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define CHECK(condition) do { if (!(condition)) return __LINE__; } while (0)
#define OK(call) CHECK((call) == 0)

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t condition = PTHREAD_COND_INITIALIZER;
static pthread_key_t key;
static int produced;
static int destructor_calls;

static void destroy_value(void *value) {
    if (value == &produced)
        ++destructor_calls;
}

static void *producer(void *argument) {
    char tokens[] = "worker,last";
    if (pthread_getspecific(key) != NULL || errno != 0 ||
        pthread_setspecific(key, &produced) != 0 ||
        strcmp(strtok(tokens, ","), "worker") != 0)
        _exit(70);
    if (ftrylockfile(stdout) == 0)
        _exit(71);
    errno = EDOM;
    srand(7);
    int first = rand();
    if (sched_yield() != 0 || errno != EDOM || strcmp(strtok(NULL, ","), "last") != 0)
        _exit(72);
    srand(7);
    if (rand() != first || pthread_mutex_lock(&mutex) != 0)
        _exit(73);
    produced = 42;
    if (pthread_cond_signal(&condition) != 0 || pthread_mutex_unlock(&mutex) != 0)
        _exit(74);
    return argument;
}

static void *return_argument(void *argument) {
    if (sched_yield() != 0)
        return NULL;
    return argument;
}

static void *wait_forever(void *argument) {
    flockfile(stdout); /* main must be able to exit while a worker holds a libc lock */
    if (pthread_mutex_lock(&mutex) != 0)
        _exit(95);
    produced = 1;
    if (pthread_cond_signal(&condition) != 0)
        _exit(96);
    for (;;)
        if (pthread_cond_wait(&condition, &mutex) != 0)
            _exit(97);
    return argument;
}

static void *exit_worker(void *argument) {
    (void)argument;
    _exit(31);
}

int main(int argc, char **argv) {
    pthread_t thread;
    if (argc > 1 && strcmp(argv[1], "worker-exit") == 0) {
        OK(pthread_create(&thread, NULL, exit_worker, NULL));
        (void)pthread_join(thread, NULL);
        return 98;
    }
    if (argc > 1 && strcmp(argv[1], "main-return") == 0) {
        OK(pthread_mutex_lock(&mutex));
        OK(pthread_create(&thread, NULL, wait_forever, NULL));
        while (!produced)
            OK(pthread_cond_wait(&condition, &mutex));
        OK(pthread_mutex_unlock(&mutex));
        return 29;
    }

    OK(pthread_key_create(&key, destroy_value));
    CHECK(pthread_equal(pthread_self(), pthread_self()));
    OK(pthread_setspecific(key, &thread));

    char tokens[] = "main,last";
    CHECK(strcmp(strtok(tokens, ","), "main") == 0);
    errno = ERANGE;
    srand(11);
    int first = rand();
    int second = rand();
    srand(11);
    CHECK(rand() == first);

    OK(pthread_mutex_lock(&mutex));
    CHECK(pthread_mutex_trylock(&mutex) == EBUSY);
    flockfile(stdout);
    OK(pthread_create(&thread, NULL, producer, &mutex));
    CHECK(!pthread_equal(pthread_self(), thread));
    while (!produced)
        OK(pthread_cond_wait(&condition, &mutex));
    OK(pthread_mutex_unlock(&mutex));

    funlockfile(stdout);
    CHECK(produced == 42 && errno == ERANGE);
    CHECK(rand() == second && strcmp(strtok(NULL, ","), "last") == 0);
    void *result = NULL;
    OK(pthread_join(thread, &result));
    CHECK(result == &mutex && destructor_calls == 1);
    CHECK(pthread_getspecific(key) == &thread);
    OK(pthread_key_delete(key));

    pthread_mutexattr_t attributes;
    pthread_mutex_t recursive;
    OK(pthread_mutexattr_init(&attributes));
    OK(pthread_mutexattr_settype(&attributes, PTHREAD_MUTEX_RECURSIVE));
    OK(pthread_mutex_init(&recursive, &attributes));
    OK(pthread_mutexattr_destroy(&attributes));
    OK(pthread_mutex_lock(&recursive));
    OK(pthread_mutex_lock(&recursive));
    OK(pthread_mutex_unlock(&recursive));
    CHECK(pthread_mutex_destroy(&recursive) == EBUSY);
    OK(pthread_mutex_unlock(&recursive));
    OK(pthread_mutex_destroy(&recursive));

    pthread_condattr_t clock_attribute;
    pthread_cond_t timer;
    OK(pthread_condattr_init(&clock_attribute));
    OK(pthread_condattr_setclock(&clock_attribute, CLOCK_MONOTONIC));
    OK(pthread_cond_init(&timer, &clock_attribute));
    OK(pthread_condattr_destroy(&clock_attribute));
    struct timespec deadline;
    OK(clock_gettime(CLOCK_MONOTONIC, &deadline));
    deadline.tv_nsec += 2 * 1000 * 1000;
    if (deadline.tv_nsec >= 1000000000) {
        ++deadline.tv_sec;
        deadline.tv_nsec -= 1000000000;
    }
    OK(pthread_mutex_lock(&mutex));
    CHECK(pthread_cond_timedwait(&timer, &mutex, &deadline) == ETIMEDOUT);
    OK(pthread_mutex_unlock(&mutex));
    OK(pthread_cond_destroy(&timer));

    pthread_attr_t thread_attribute;
    OK(pthread_attr_init(&thread_attribute));
    OK(pthread_attr_setstacksize(&thread_attribute, 8192));
    pthread_t workers[12];
    for (size_t index = 0; index < sizeof(workers) / sizeof(workers[0]); ++index)
        OK(pthread_create(&workers[index], &thread_attribute, return_argument, &workers[index]));
    for (size_t index = 0; index < sizeof(workers) / sizeof(workers[0]); ++index) {
        OK(pthread_join(workers[index], &result));
        CHECK(result == &workers[index]);
    }
    OK(pthread_attr_destroy(&thread_attribute));

    OK(pthread_create(&thread, NULL, return_argument, NULL));
    OK(pthread_detach(thread));
    OK(sched_yield());
    OK(sched_yield());
    OK(sched_yield());
    CHECK(pthread_join(thread, NULL) == ESRCH);
    OK(pthread_cond_destroy(&condition));
    OK(pthread_mutex_destroy(&mutex));

    flockfile(stdout);
    OK(ftrylockfile(stdout));
    CHECK(puts("threads: ok") >= 0);
    funlockfile(stdout);
    funlockfile(stdout);
    return 0;
}
