#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <sched.h>
#include <semaphore.h>
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

static void open_pipe_writer(void) {
    int fd = open("/thread-pipe", O_WRONLY);
    if (fd < 0 || write(fd, "x", 1) != 1)
        _exit(94);
    /* Leave the descriptor open for process teardown. */
}

static void *wait_forever(void *argument) {
    open_pipe_writer();
    if (argument != NULL)
        flockfile(stdout);
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
    open_pipe_writer();
    if (argument != NULL)
        exit(32);
    _exit(31);
}

/* Opens the entropy device while the host holds the device lock: the open suspends with the
   VFS namespace lock held, until exit stops this thread. */
static void *open_locked_device(void *argument) {
    (void)argument;
    (void)open("/devices/random/source", O_RDONLY);
    return NULL;
}

static pthread_once_t once = PTHREAD_ONCE_INIT;
static int initialized;

static void initialize_once(void) {
    ++initialized;
    if (sched_yield() != 0)
        _exit(100);
    ++initialized;
}

static void *call_once(void *argument) {
    if (pthread_once(&once, initialize_once) != 0 || initialized != 2)
        _exit(101);
    return argument;
}

static pthread_t orphan;
static int child_finished;

static void *orphan_worker(void *argument) {
    if (pthread_mutex_lock(&mutex) != 0)
        _exit(102);
    child_finished = 1;
    if (pthread_mutex_unlock(&mutex) != 0)
        _exit(103);
    return argument;
}

static void *creator(void *argument) {
    if (pthread_create(&orphan, NULL, orphan_worker, NULL) != 0 ||
        pthread_setspecific(key, &produced) != 0)
        _exit(104);
    pthread_exit(argument);
}

static int order[3];
static size_t order_count;

static void *record_lock_order(void *argument) {
    int id = *(int *)argument;
    if (id == 0 && sched_yield() != 0)
        _exit(105);
    if (pthread_mutex_lock(&mutex) != 0)
        _exit(106);
    order[order_count++] = id;
    if (pthread_mutex_unlock(&mutex) != 0)
        _exit(107);
    return NULL;
}

static pthread_cond_t timed_condition = PTHREAD_COND_INITIALIZER;
static int timed_woken;

static void *timed_waiter(void *argument) {
    (void)argument;
    struct timespec deadline;
    if (clock_gettime(CLOCK_MONOTONIC, &deadline) != 0 ||
        pthread_mutex_lock(&mutex) != 0)
        _exit(108);
    ++deadline.tv_sec;
    if (pthread_cond_timedwait(&timed_condition, &mutex, &deadline) != 0)
        _exit(109);
    ++timed_woken;
    if (pthread_mutex_unlock(&mutex) != 0)
        _exit(110);
    return NULL;
}

static void *print_lines(void *argument) {
    for (int line = 0; line < 4; ++line) {
        flockfile(stdout);
        if (printf("%s\n", (char *)argument) < 0 || sched_yield() != 0)
            _exit(111);
        funlockfile(stdout);
    }
    return NULL;
}

static int check_wait_queues(void) {
    pthread_t workers[3];
    int ids[] = {0, 1, 2};
    OK(pthread_mutex_lock(&mutex));
    for (size_t i = 0; i < 3; ++i)
        OK(pthread_create(&workers[i], NULL, record_lock_order, &ids[i]));
    OK(sched_yield());
    OK(sched_yield());
    OK(pthread_mutex_unlock(&mutex));
    CHECK(pthread_mutex_trylock(&mutex) == EBUSY);
    for (size_t i = 0; i < 3; ++i)
        OK(pthread_join(workers[i], NULL));
    CHECK(order_count == 3 && order[0] == 1 && order[1] == 2 && order[2] == 0);

    for (size_t i = 0; i < 2; ++i)
        OK(pthread_create(&workers[i], NULL, timed_waiter, NULL));
    OK(sched_yield());
    OK(pthread_mutex_lock(&mutex));
    OK(pthread_cond_signal(&timed_condition));
    OK(pthread_mutex_unlock(&mutex));
    OK(sched_yield());
    CHECK(timed_woken == 1);
    CHECK(pthread_cond_destroy(&timed_condition) == EBUSY);
    OK(pthread_cond_signal(&timed_condition));
    OK(pthread_cond_destroy(&timed_condition));
    for (size_t i = 0; i < 2; ++i)
        OK(pthread_join(workers[i], NULL));
    CHECK(timed_woken == 2);
    return 0;
}

static sem_t semaphore;
static int semaphore_order[2];
static int semaphore_taken;

/* Blocks on the semaphore and records the order in which waiters got their unit. */
static void *take_semaphore(void *argument) {
    if (sem_wait(&semaphore) != 0)
        return NULL;
    semaphore_order[semaphore_taken++] = *(int *)argument;
    return argument;
}

static int add_milliseconds(clockid_t clock, struct timespec *deadline, long milliseconds) {
    if (clock_gettime(clock, deadline) != 0)
        return -1;
    deadline->tv_nsec += milliseconds * 1000 * 1000;
    while (deadline->tv_nsec >= 1000000000) {
        ++deadline->tv_sec;
        deadline->tv_nsec -= 1000000000;
    }
    return 0;
}

static int check_semaphores(void) {
    int value = -1;
    CHECK(sem_init(&semaphore, 1, 0) == -1 && errno == ENOSYS);
    CHECK(sem_init(&semaphore, 0, (unsigned int)SEM_VALUE_MAX + 1u) == -1 && errno == EINVAL);
    OK(sem_init(&semaphore, 0, 1));
    OK(sem_getvalue(&semaphore, &value));
    CHECK(value == 1);
    /* A unit is available: no deadline check, even for a malformed one. */
    struct timespec deadline = {0, -1};
    OK(sem_timedwait(&semaphore, &deadline));
    CHECK(sem_trywait(&semaphore) == -1 && errno == EAGAIN);
    CHECK(sem_timedwait(&semaphore, &deadline) == -1 && errno == EINVAL);
    CHECK(sem_clockwait(&semaphore, CLOCK_MONOTONIC, NULL) == -1 && errno == EINVAL);
    CHECK(add_milliseconds(CLOCK_MONOTONIC, &deadline, 2) == 0);
    CHECK(sem_clockwait(&semaphore, CLOCK_MONOTONIC, &deadline) == -1 && errno == ETIMEDOUT);
    CHECK(add_milliseconds(CLOCK_REALTIME, &deadline, 2) == 0);
    CHECK(sem_timedwait(&semaphore, &deadline) == -1 && errno == ETIMEDOUT);

    /* Two blocked waiters get their units in arrival order, one per post. */
    pthread_t first, second;
    int first_name = 1, second_name = 2;
    OK(pthread_create(&first, NULL, take_semaphore, &first_name));
    OK(sched_yield());
    OK(pthread_create(&second, NULL, take_semaphore, &second_name));
    OK(sched_yield());
    CHECK(semaphore_taken == 0);
    OK(sem_post(&semaphore));
    void *result = NULL;
    OK(pthread_join(first, &result));
    CHECK(result == &first_name && semaphore_taken == 1);
    OK(sem_post(&semaphore));
    OK(pthread_join(second, &result));
    CHECK(result == &second_name && semaphore_taken == 2);
    CHECK(semaphore_order[0] == 1 && semaphore_order[1] == 2);
    OK(sem_getvalue(&semaphore, &value));
    CHECK(value == 0);
    OK(sem_destroy(&semaphore));

    OK(sem_init(&semaphore, 0, SEM_VALUE_MAX));
    CHECK(sem_post(&semaphore) == -1 && errno == EOVERFLOW);
    OK(sem_getvalue(&semaphore, &value));
    CHECK(value == SEM_VALUE_MAX);
    OK(sem_destroy(&semaphore));

    CHECK(sem_open("/named", 0) == SEM_FAILED && errno == ENOSYS);
    CHECK(sem_unlink("/named") == -1 && errno == ENOSYS);
    return 0;
}

static pthread_once_t exit_once = PTHREAD_ONCE_INIT;
static int exit_once_runs;

/* The first run yields, so a second caller queues on the control, and then leaves its
   thread through pthread_exit. */
static void initialize_then_exit(void) {
    if (++exit_once_runs == 1) {
        if (sched_yield() != 0)
            _exit(102);
        pthread_exit(&exit_once_runs);
    }
}

static void *call_exit_once(void *argument) {
    (void)pthread_once(&exit_once, initialize_then_exit);
    return argument;
}

/* A routine that leaves through pthread_exit leaves the control as if never called: the
   caller queued on it runs the routine again instead of waiting forever. */
static int check_once_exit(void) {
    pthread_t thread;
    OK(pthread_create(&thread, NULL, call_exit_once, NULL));
    OK(sched_yield());
    CHECK(exit_once_runs == 1);
    OK(pthread_once(&exit_once, initialize_then_exit));
    CHECK(exit_once_runs == 2);
    void *result = NULL;
    OK(pthread_join(thread, &result));
    CHECK(result == &exit_once_runs);
    OK(pthread_once(&exit_once, initialize_then_exit));
    CHECK(exit_once_runs == 2);
    return 0;
}

static pthread_rwlock_t rwlock = PTHREAD_RWLOCK_INITIALIZER;
static int readers_inside;
static int most_readers_inside;
static int writer_entered;

/* Holds the read lock across a yield, so other threads run while it is inside. */
static void *read_shared(void *argument) {
    if (pthread_rwlock_rdlock(&rwlock) != 0)
        return NULL;
    if (++readers_inside > most_readers_inside)
        most_readers_inside = readers_inside;
    int yielded = sched_yield();
    --readers_inside;
    if (pthread_rwlock_unlock(&rwlock) != 0 || yielded != 0)
        return NULL;
    return argument;
}

static void *write_exclusive(void *argument) {
    if (pthread_rwlock_wrlock(&rwlock) != 0)
        return NULL;
    int alone = readers_inside == 0;
    writer_entered = 1;
    if (pthread_rwlock_unlock(&rwlock) != 0 || !alone)
        return NULL;
    return argument;
}

/* Runs while main holds the write lock. */
static void *time_out_on_rwlock(void *argument) {
    struct timespec deadline = {0, -1};
    if (pthread_rwlock_tryrdlock(&rwlock) != EBUSY ||
        pthread_rwlock_trywrlock(&rwlock) != EBUSY ||
        pthread_rwlock_timedwrlock(&rwlock, &deadline) != EINVAL ||
        add_milliseconds(CLOCK_REALTIME, &deadline, 2) != 0 ||
        pthread_rwlock_timedrdlock(&rwlock, &deadline) != ETIMEDOUT ||
        add_milliseconds(CLOCK_MONOTONIC, &deadline, 2) != 0 ||
        pthread_rwlock_clockwrlock(&rwlock, CLOCK_MONOTONIC, &deadline) != ETIMEDOUT ||
        pthread_rwlock_clockrdlock(&rwlock, (clockid_t)-1, &deadline) != EINVAL)
        return NULL;
    return argument;
}

static int check_rwlocks(void) {
    /* Read locks are shared and recursive; a malformed deadline is ignored when the lock
       is free. */
    struct timespec malformed = {0, -1};
    OK(pthread_rwlock_rdlock(&rwlock));
    OK(pthread_rwlock_rdlock(&rwlock));
    OK(pthread_rwlock_tryrdlock(&rwlock));
    OK(pthread_rwlock_timedrdlock(&rwlock, &malformed));
    CHECK(pthread_rwlock_trywrlock(&rwlock) == EBUSY);
    CHECK(pthread_rwlock_destroy(&rwlock) == EBUSY);
    for (int i = 0; i < 4; ++i)
        OK(pthread_rwlock_unlock(&rwlock));
    CHECK(pthread_rwlock_unlock(&rwlock) == EPERM);

    /* The write lock is exclusive, even against its holder. */
    OK(pthread_rwlock_wrlock(&rwlock));
    CHECK(pthread_rwlock_rdlock(&rwlock) == EDEADLK);
    CHECK(pthread_rwlock_wrlock(&rwlock) == EDEADLK);
    CHECK(pthread_rwlock_tryrdlock(&rwlock) == EBUSY);
    pthread_t timer;
    int timer_name = 0;
    void *result = NULL;
    OK(pthread_create(&timer, NULL, time_out_on_rwlock, &timer_name));
    OK(pthread_join(timer, &result));
    CHECK(result == &timer_name);

    /* Two readers and a writer queue behind main's write lock. Once it is released both
       readers are inside together, and the writer enters only after they leave. */
    pthread_t workers[3];
    int names[3] = {1, 2, 3};
    OK(pthread_create(&workers[0], NULL, read_shared, &names[0]));
    OK(pthread_create(&workers[1], NULL, read_shared, &names[1]));
    OK(pthread_create(&workers[2], NULL, write_exclusive, &names[2]));
    OK(sched_yield());
    CHECK(most_readers_inside == 0 && !writer_entered);
    OK(pthread_rwlock_unlock(&rwlock));
    for (int i = 0; i < 3; ++i) {
        OK(pthread_join(workers[i], &result));
        CHECK(result == &names[i]);
    }
    CHECK(most_readers_inside == 2 && writer_entered);

    pthread_rwlockattr_t attributes;
    int shared = -1;
    OK(pthread_rwlockattr_init(&attributes));
    OK(pthread_rwlockattr_getpshared(&attributes, &shared));
    CHECK(shared == PTHREAD_PROCESS_PRIVATE);
    CHECK(pthread_rwlockattr_setpshared(&attributes, PTHREAD_PROCESS_SHARED) == ENOTSUP);
    pthread_rwlock_t initialized;
    OK(pthread_rwlock_init(&initialized, &attributes));
    OK(pthread_rwlockattr_destroy(&attributes));
    OK(pthread_rwlock_wrlock(&initialized));
    OK(pthread_rwlock_unlock(&initialized));
    OK(pthread_rwlock_destroy(&initialized));
    OK(pthread_rwlock_destroy(&rwlock));
    return 0;
}

static void announce_destructor(void *value) {
    (void)value;
    (void)write(STDOUT_FILENO, "destructor\n", 11);
}

int main(int argc, char **argv) {
    pthread_t thread;
    if (argc > 1 && strcmp(argv[1], "exit-under-lock") == 0) {
        /* exit() stops the worker, which gives the namespace lock back, before it closes
           and flushes the file, which need that lock. */
        FILE *file = fopen("/thread-locked.txt", "w");
        CHECK(file != NULL && fputs("flushed", file) >= 0);
        OK(pthread_create(&thread, NULL, open_locked_device, NULL));
        OK(sched_yield());
        exit(33);
    }
    if (argc > 1 && strcmp(argv[1], "print-contention") == 0) {
        OK(pthread_create(&thread, NULL, print_lines, "worker"));
        OK(sched_yield());
        print_lines("main");
        OK(pthread_join(thread, NULL));
        return 0;
    }
    if (argc > 1 && (strcmp(argv[1], "worker-exit") == 0 ||
                     strcmp(argv[1], "worker-exit-flush") == 0)) {
        void *flush = strcmp(argv[1], "worker-exit-flush") == 0 ? &mutex : NULL;
        CHECK(fputs("partial line", stdout) >= 0);
        OK(pthread_create(&thread, NULL, exit_worker, flush));
        (void)pthread_join(thread, NULL);
        return 98;
    }
    if (argc > 1 && strcmp(argv[1], "main-pthread-exit") == 0) {
        /* Unsupported on main: it aborts before running any destructor. */
        OK(pthread_key_create(&key, announce_destructor));
        OK(pthread_setspecific(key, &key));
        pthread_exit(NULL);
    }
    if (argc > 1 && (strcmp(argv[1], "main-return") == 0 ||
                     strcmp(argv[1], "locked-stdout") == 0)) {
        CHECK(fputs("partial line", stdout) >= 0);
        FILE *file = fopen("/thread-buffered.txt", "w");
        CHECK(file != NULL && fputs("buffered file", file) >= 0);
        OK(pthread_mutex_lock(&mutex));
        void *lock_stdout = strcmp(argv[1], "locked-stdout") == 0 ? &mutex : NULL;
        OK(pthread_create(&thread, NULL, wait_forever, lock_stdout));
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
    OK(pthread_mutex_lock(&mutex));
    OK(pthread_create(&thread, NULL, creator, &mutex));
    OK(pthread_join(thread, &result));
    CHECK(result == &mutex && destructor_calls == 2);
    CHECK(pthread_detach(orphan) == EINVAL && !child_finished);
    OK(pthread_mutex_unlock(&mutex));
    OK(sched_yield());
    CHECK(child_finished && pthread_detach(orphan) == ESRCH);
    OK(pthread_key_delete(key));

    OK(pthread_create(&thread, NULL, call_once, NULL));
    OK(sched_yield());
    call_once(NULL);
    OK(pthread_join(thread, NULL));

    int queue_status = check_wait_queues();
    if (queue_status != 0)
        return queue_status;
    int semaphore_status = check_semaphores();
    if (semaphore_status != 0)
        return semaphore_status;
    int once_exit_status = check_once_exit();
    if (once_exit_status != 0)
        return once_exit_status;
    int rwlock_status = check_rwlocks();
    if (rwlock_status != 0)
        return rwlock_status;

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
    clockid_t clock;
    OK(pthread_condattr_getclock(&clock_attribute, &clock));
    CHECK(clock == CLOCK_MONOTONIC);
    OK(pthread_condattr_setclock(&clock_attribute, CLOCK_REALTIME));
    OK(pthread_condattr_getclock(&clock_attribute, &clock));
    CHECK(clock == CLOCK_REALTIME);
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
