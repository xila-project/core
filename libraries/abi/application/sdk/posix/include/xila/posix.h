/* Calls of the Xila POSIX layer that picolibc's headers do not declare. */
#ifndef XILA_POSIX_LAYER_H
#define XILA_POSIX_LAYER_H

#include <fcntl.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/random.h>
#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

int openat(int dirfd, const char *path, int flags, ...);
int mkdirat(int dirfd, const char *path, mode_t mode);
int unlinkat(int dirfd, const char *path, int flags);
int rename(const char *from, const char *to);
int renameat(int from_dirfd, const char *from, int to_dirfd, const char *to);
int fstatat(int dirfd, const char *path, struct stat *output, int flags);
ssize_t pread(int fd, void *buffer, size_t size, off_t offset);
ssize_t pwrite(int fd, const void *buffer, size_t size, off_t offset);

#ifdef __cplusplus
}
#endif

#endif
