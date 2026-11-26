/* Clocks, sleeping and entropy. */
#include <limits.h>
#include <sys/time.h>
#include <sys/random.h>
#include <time.h>
#include <unistd.h>

#include "internal.h"

#define NANOSECONDS 1000000000ULL

static int clock_identifier(clockid_t clock, uint32_t *native) {
    switch (clock) {
    case CLOCK_REALTIME:
        *native = XILA_CLOCK_REALTIME;
        return 0;
    case CLOCK_MONOTONIC:
        *native = XILA_CLOCK_MONOTONIC;
        return 0;
    default:
        return xila_fail_errno(EINVAL);
    }
}

int clock_gettime(clockid_t clock, struct timespec *output) {
    uint32_t native;
    if (clock_identifier(clock, &native) != 0)
        return -1;
    if (output == NULL)
        return xila_fail_errno(EFAULT);
    if (native == XILA_CLOCK_REALTIME) {
        int64_t nanoseconds = 0;
        XilaStatus status = xila_time_realtime(&nanoseconds);
        if (status != XILA_STATUS_OK)
            return xila_fail(status);
        int64_t seconds = nanoseconds / (int64_t)NANOSECONDS;
        int64_t rest = nanoseconds % (int64_t)NANOSECONDS;
        if (rest < 0) {
            rest += (int64_t)NANOSECONDS;
            --seconds;
        }
        output->tv_sec = (time_t)seconds;
        output->tv_nsec = (long)rest;
        return 0;
    }
    uint64_t nanoseconds = 0;
    XilaStatus status = xila_time_monotonic(&nanoseconds);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    output->tv_sec = (time_t)(nanoseconds / NANOSECONDS);
    output->tv_nsec = (long)(nanoseconds % NANOSECONDS);
    return 0;
}

int clock_getres(clockid_t clock, struct timespec *output) {
    uint32_t native = 0;
    if (clock_identifier(clock, &native) != 0)
        return -1;
    uint64_t nanoseconds = 0;
    XilaStatus status = xila_time_resolution(native, &nanoseconds);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    if (output != NULL) {
        output->tv_sec = (time_t)(nanoseconds / NANOSECONDS);
        output->tv_nsec = (long)(nanoseconds % NANOSECONDS);
    }
    return 0;
}

/* Sleeps are not interruptible: Xila delivers no signals, so `remaining` is zeroed. */
int nanosleep(const struct timespec *request, struct timespec *remaining) {
    if (request == NULL)
        return xila_fail_errno(EFAULT);
    if (request->tv_sec < 0 || request->tv_nsec < 0 || request->tv_nsec >= (long)NANOSECONDS)
        return xila_fail_errno(EINVAL);
    uint64_t seconds = (uint64_t)request->tv_sec;
    uint64_t total = UINT64_MAX;
    if (seconds <= (UINT64_MAX - (uint64_t)request->tv_nsec) / NANOSECONDS)
        total = seconds * NANOSECONDS + (uint64_t)request->tv_nsec;
    XilaStatus status = xila_time_sleep(total);
    if (remaining != NULL) {
        remaining->tv_sec = 0;
        remaining->tv_nsec = 0;
    }
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

unsigned int sleep(unsigned int seconds) {
    struct timespec request = {.tv_sec = (time_t)seconds, .tv_nsec = 0};
    return nanosleep(&request, NULL) == 0 ? 0 : seconds;
}

int usleep(useconds_t microseconds) {
    struct timespec request = {.tv_sec = (time_t)(microseconds / 1000000U),
                               .tv_nsec = (long)(microseconds % 1000000U) * 1000L};
    return nanosleep(&request, NULL);
}

int gettimeofday(struct timeval *output, void *timezone) {
    (void)timezone;
    struct timespec now;
    if (clock_gettime(CLOCK_REALTIME, &now) != 0)
        return -1;
    if (output != NULL) {
        output->tv_sec = now.tv_sec;
        output->tv_usec = now.tv_nsec / 1000;
    }
    return 0;
}

time_t time(time_t *output) {
    struct timespec now;
    if (clock_gettime(CLOCK_REALTIME, &now) != 0)
        return (time_t)-1;
    if (output != NULL)
        *output = now.tv_sec;
    return now.tv_sec;
}

ssize_t getrandom(void *buffer, size_t size, unsigned int flags) {
    (void)flags; /* the host source never runs dry, so GRND_NONBLOCK needs no handling */
    if (size > SSIZE_MAX)
        return xila_fail_errno(EINVAL);
    XilaStatus status = xila_random_fill(buffer, size);
    return status == XILA_STATUS_OK ? (ssize_t)size : xila_fail(status);
}

int getentropy(void *buffer, size_t size) {
    if (size > 256)
        return xila_fail_errno(EINVAL); /* POSIX.1-2024 */
    return getrandom(buffer, size, 0) < 0 ? -1 : 0;
}
