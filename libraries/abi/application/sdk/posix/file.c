/* Files and directories over the capability ABI. */
#include <dirent.h>
#include <fcntl.h>
#include <limits.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "internal.h"
#include <xila/posix.h>

#define NANOSECONDS 1000000000LL

/* ---- open and close ------------------------------------------------------------ */

static uint32_t open_flags(int flags) {
    uint32_t native = 0;
    switch (flags & O_ACCMODE) {
    case O_RDONLY:
        native = XILA_OPEN_READ;
        break;
    case O_WRONLY:
        native = XILA_OPEN_WRITE;
        break;
    default:
        native = XILA_OPEN_READ | XILA_OPEN_WRITE;
        break;
    }
    if (flags & O_CREAT)
        native |= XILA_OPEN_CREATE;
    if (flags & O_EXCL)
        native |= XILA_OPEN_EXCLUSIVE;
    if (flags & O_TRUNC)
        native |= XILA_OPEN_TRUNCATE;
    if (flags & O_APPEND)
        native |= XILA_OPEN_APPEND;
    if (flags & O_NONBLOCK)
        native |= XILA_OPEN_NON_BLOCKING;
    return native;
}

/* A directory descriptor is read-only: callers have rejected every write access mode. */
static int open_directory(const struct xila_path *path) {
    XilaDirectory *directory = NULL;
    XilaStatus status = xila_directory_open(path->directory, path->relative, path->length,
                                            XILA_RIGHT_READ, &directory);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    int fd = xila_fd_allocate(directory, XILA_FD_DIRECTORY, 0);
    if (fd < 0)
        (void)xila_directory_close(directory);
    return fd;
}

int openat(int dirfd, const char *path, int flags, ...) {
    char buffer[XILA_PATH_MAX];
    struct xila_path resolved;
    if (xila_path_resolve(dirfd, path, buffer, &resolved) != 0)
        return -1;
    int read_only = (flags & O_ACCMODE) == O_RDONLY && !(flags & (O_CREAT | O_TRUNC));
    if (flags & O_DIRECTORY)
        return (flags & O_ACCMODE) == O_RDONLY ? open_directory(&resolved)
                                               : xila_fail_errno(EISDIR);

    XilaFile *file = NULL;
    XilaStatus status = xila_file_open(resolved.directory, resolved.relative, resolved.length,
                                       open_flags(flags), &file);
    /* POSIX allows a read-only open of a directory. The root has an empty relative path,
       which a file open rejects as an invalid argument. */
    if (read_only && (status == XILA_STATUS_IS_A_DIRECTORY ||
                      (status == XILA_STATUS_INVALID_ARGUMENT && resolved.length == 0)))
        return open_directory(&resolved);
    if (status == XILA_STATUS_INVALID_ARGUMENT && resolved.length == 0)
        return xila_fail_errno(EISDIR); /* the root opened for writing */
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    int fd = xila_fd_allocate(file, XILA_FD_FILE, 0);
    if (fd < 0)
        (void)xila_file_close(file);
    return fd;
}

/* The mode of a created file is accepted and ignored: Core's open has no mode argument. */
int open(const char *path, int flags, ...) {
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list arguments;
        va_start(arguments, flags);
        mode = (mode_t)va_arg(arguments, int);
        va_end(arguments);
    }
    return openat(AT_FDCWD, path, flags, mode);
}

int creat(const char *path, mode_t mode) { return open(path, O_WRONLY | O_CREAT | O_TRUNC, mode); }

int close(int fd) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return -1;
    void *object = entry->object;
    uint8_t kind = entry->kind;
    uint8_t flags = entry->flags;
    entry->object = NULL; /* the descriptor is gone even when the ABI reports an error */
    if (flags & XILA_FD_BORROWED)
        return 0;
    XilaStatus status =
        kind == XILA_FD_FILE ? xila_file_close(object) : xila_directory_close(object);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

/* ---- data transfer ------------------------------------------------------------- */

static XilaFile *file_of(int fd) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return NULL;
    if (entry->kind != XILA_FD_FILE) {
        errno = EISDIR;
        return NULL;
    }
    return entry->object;
}

ssize_t read(int fd, void *buffer, size_t size) {
    XilaFile *file = file_of(fd);
    if (file == NULL)
        return -1;
    if (size > SSIZE_MAX)
        return xila_fail_errno(EINVAL);
    size_t count = 0;
    XilaStatus status = xila_file_read(file, buffer, size, &count);
    return status == XILA_STATUS_OK ? (ssize_t)count : xila_fail(status);
}

ssize_t write(int fd, const void *buffer, size_t size) {
    XilaFile *file = file_of(fd);
    if (file == NULL)
        return -1;
    if (size > SSIZE_MAX)
        return xila_fail_errno(EINVAL);
    size_t count = 0;
    XilaStatus status = xila_file_write(file, buffer, size, &count);
    return status == XILA_STATUS_OK ? (ssize_t)count : xila_fail(status);
}

off_t lseek(int fd, off_t offset, int whence) {
    XilaFile *file = file_of(fd);
    if (file == NULL)
        return -1;
    uint32_t native;
    switch (whence) {
    case SEEK_SET:
        native = XILA_SEEK_START;
        break;
    case SEEK_CUR:
        native = XILA_SEEK_CURRENT;
        break;
    case SEEK_END:
        native = XILA_SEEK_END;
        break;
    default:
        return xila_fail_errno(EINVAL);
    }
    uint64_t position = 0;
    XilaStatus status = xila_file_seek(file, offset, native, &position);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    if (position > (uint64_t)INT64_MAX)
        return xila_fail_errno(EOVERFLOW);
    return (off_t)position;
}

/* The ABI has no positional I/O yet: emulate it by seeking and restoring the cursor.
   The pair is not atomic against other users of the same descriptor. */
static ssize_t positional(int fd, void *buffer, const void *source, size_t size, off_t offset) {
    XilaFile *file = file_of(fd);
    if (file == NULL)
        return -1;
    if (offset < 0 || size > SSIZE_MAX)
        return xila_fail_errno(EINVAL);
    uint64_t saved = 0, ignored = 0;
    XilaStatus status = xila_file_seek(file, 0, XILA_SEEK_CURRENT, &saved);
    if (status != XILA_STATUS_OK)
        return xila_fail(status); /* the cursor is unknown and nothing moved: leave it */
    status = xila_file_seek(file, offset, XILA_SEEK_START, &ignored);
    size_t count = 0;
    if (status == XILA_STATUS_OK)
        status = buffer != NULL ? xila_file_read(file, buffer, size, &count)
                                : xila_file_write(file, source, size, &count);
    /* The cursor may have moved, even when the transfer failed: put it back. */
    XilaStatus restore = xila_file_seek(file, (int64_t)saved, XILA_SEEK_START, &ignored);
    if (status == XILA_STATUS_OK)
        status = restore;
    return status == XILA_STATUS_OK ? (ssize_t)count : xila_fail(status);
}

ssize_t pread(int fd, void *buffer, size_t size, off_t offset) {
    return positional(fd, buffer, NULL, size, offset);
}

ssize_t pwrite(int fd, const void *buffer, size_t size, off_t offset) {
    return positional(fd, NULL, buffer, size, offset);
}

int fsync(int fd) {
    XilaFile *file = file_of(fd);
    if (file == NULL)
        return -1;
    XilaStatus status = xila_file_flush(file);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

int fdatasync(int fd) { return fsync(fd); }

int ftruncate(int fd, off_t length) {
    XilaFile *file = file_of(fd);
    if (file == NULL)
        return -1;
    if (length < 0)
        return xila_fail_errno(EINVAL);
    XilaStatus status = xila_file_set_size(file, (uint64_t)length);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}

/* The ABI has no terminal query yet. */
int isatty(int fd) {
    if (xila_fd_get(fd) == NULL)
        return 0;
    errno = ENOTTY;
    return 0;
}

/* ---- metadata ------------------------------------------------------------------ */

static void set_time(struct timespec *output, int64_t nanoseconds) {
    int64_t seconds = nanoseconds / NANOSECONDS;
    int64_t rest = nanoseconds % NANOSECONDS;
    if (rest < 0) {
        rest += NANOSECONDS;
        --seconds;
    }
    output->tv_sec = (time_t)seconds;
    output->tv_nsec = (long)rest;
}

static mode_t kind_mode(uint32_t kind) {
    switch (kind) {
    case XILA_KIND_DIRECTORY:
        return S_IFDIR;
    case XILA_KIND_BLOCK_DEVICE:
        return S_IFBLK;
    case XILA_KIND_CHARACTER_DEVICE:
        return S_IFCHR;
    case XILA_KIND_PIPE:
        return S_IFIFO;
    case XILA_KIND_SOCKET:
        return S_IFSOCK;
    case XILA_KIND_SYMLINK:
        return S_IFLNK;
    default:
        return S_IFREG;
    }
}

static void fill_stat(struct stat *output, const XilaStatistics *native) {
    memset(output, 0, sizeof(*output));
    output->st_mode = kind_mode(native->kind) | (native->permissions & 07777);
    output->st_uid = native->owner_user;
    output->st_gid = native->owner_group;
    output->st_size = (off_t)native->file_size;
    output->st_nlink = (nlink_t)native->link_count;
    output->st_ino = (ino_t)native->inode;
    set_time(&output->st_atim, native->access_nanoseconds);
    set_time(&output->st_mtim, native->modification_nanoseconds);
    set_time(&output->st_ctim, native->status_change_nanoseconds);
}

int fstat(int fd, struct stat *output) {
    struct xila_fd *entry = xila_fd_get(fd);
    if (entry == NULL)
        return -1;
    XilaStatistics native = {.size = sizeof(native)};
    XilaStatus status = entry->kind == XILA_FD_FILE
                            ? xila_file_get_statistics(entry->object, &native)
                            : xila_directory_get_statistics(entry->object, "", 0, 0, &native);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    fill_stat(output, &native);
    return 0;
}

static int stat_path(int dirfd, const char *path, struct stat *output, uint32_t flags) {
    char buffer[XILA_PATH_MAX];
    struct xila_path resolved;
    if (xila_path_resolve(dirfd, path, buffer, &resolved) != 0)
        return -1;
    XilaStatistics native = {.size = sizeof(native)};
    XilaStatus status = xila_directory_get_statistics(resolved.directory, resolved.relative,
                                                      resolved.length, flags, &native);
    if (status != XILA_STATUS_OK)
        return xila_fail(status);
    fill_stat(output, &native);
    return 0;
}

int fstatat(int dirfd, const char *path, struct stat *output, int flags) {
    return stat_path(dirfd, path, output, (flags & AT_SYMLINK_NOFOLLOW) ? XILA_STATISTICS_NO_FOLLOW : 0);
}
int stat(const char *path, struct stat *output) { return stat_path(AT_FDCWD, path, output, 0); }
int lstat(const char *path, struct stat *output) {
    return stat_path(AT_FDCWD, path, output, XILA_STATISTICS_NO_FOLLOW);
}

/* ---- namespace ----------------------------------------------------------------- */

int mkdirat(int dirfd, const char *path, mode_t mode) {
    (void)mode;
    char buffer[XILA_PATH_MAX];
    struct xila_path resolved;
    if (xila_path_resolve(dirfd, path, buffer, &resolved) != 0)
        return -1;
    XilaStatus status = xila_directory_create(resolved.directory, resolved.relative, resolved.length);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}
int mkdir(const char *path, mode_t mode) { return mkdirat(AT_FDCWD, path, mode); }

int unlinkat(int dirfd, const char *path, int flags) {
    char buffer[XILA_PATH_MAX];
    struct xila_path resolved;
    if (xila_path_resolve(dirfd, path, buffer, &resolved) != 0)
        return -1;
    uint32_t native = 0;
    if (flags & AT_REMOVEDIR) {
        /* XILA_REMOVE_DIRECTORY also removes files: check the kind first. */
        XilaStatistics statistics = {.size = sizeof(statistics)};
        XilaStatus status = xila_directory_get_statistics(
            resolved.directory, resolved.relative, resolved.length, XILA_STATISTICS_NO_FOLLOW,
            &statistics);
        if (status != XILA_STATUS_OK)
            return xila_fail(status);
        if (statistics.kind != XILA_KIND_DIRECTORY)
            return xila_fail_errno(ENOTDIR);
        native = XILA_REMOVE_DIRECTORY;
    }
    XilaStatus status =
        xila_directory_remove(resolved.directory, resolved.relative, resolved.length, native);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}
int unlink(const char *path) { return unlinkat(AT_FDCWD, path, 0); }
int rmdir(const char *path) { return unlinkat(AT_FDCWD, path, AT_REMOVEDIR); }

int renameat(int from_dirfd, const char *from, int to_dirfd, const char *to) {
    char from_buffer[XILA_PATH_MAX], to_buffer[XILA_PATH_MAX];
    struct xila_path source, target;
    if (xila_path_resolve(from_dirfd, from, from_buffer, &source) != 0 ||
        xila_path_resolve(to_dirfd, to, to_buffer, &target) != 0)
        return -1;
    XilaStatus status = xila_directory_rename(source.directory, source.relative, source.length,
                                              target.directory, target.relative, target.length);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}
int rename(const char *from, const char *to) { return renameat(AT_FDCWD, from, AT_FDCWD, to); }

/* ---- current directory --------------------------------------------------------- */

char *getcwd(char *buffer, size_t size) {
    const char *cwd = xila_cwd();
    size_t length = strlen(cwd) + 1;
    if (buffer == NULL) {
        size = size > length ? size : length;
        buffer = malloc(size);
        if (buffer == NULL)
            return NULL;
    } else if (size == 0) {
        errno = EINVAL;
        return NULL;
    } else if (size < length) {
        errno = ERANGE;
        return NULL;
    }
    memcpy(buffer, cwd, length);
    return buffer;
}

int chdir(const char *path) {
    if (path == NULL)
        return xila_fail_errno(EFAULT);
    char absolute[XILA_PATH_MAX];
    int error = xila_path_normalize(path[0] == '/' ? "/" : xila_cwd(), path, absolute, 1);
    if (error != 0)
        return xila_fail_errno(error);
    struct stat information;
    if (stat(absolute, &information) != 0)
        return -1;
    if (!S_ISDIR(information.st_mode))
        return xila_fail_errno(ENOTDIR);
    return xila_cwd_set(absolute) == 0 ? 0 : -1;
}

/* ---- directory streams --------------------------------------------------------- */

/* DIR is opaque to applications: the stream keeps the listing and one entry, plus the
   hash of the directory's absolute path that seeds d_ino. */
struct xila_dir {
    XilaDirectoryIterator *iterator;
    uint64_t seed;
    struct dirent entry;
};

/* The directory iterator reports a name and a kind, not an inode, and a statistics call
   per entry would need the directory's path kept for the life of the stream. d_ino is
   therefore a stable nonzero identifier derived from the absolute path (64-bit FNV-1a): the
   same entry of the same directory always has the same value, which is what readdir
   callers need (zero marks a deleted entry on some systems), but it is not st_ino. */
#define FNV_OFFSET_BASIS 14695981039346656037ULL
#define FNV_PRIME 1099511628211ULL

static uint64_t fnv(uint64_t hash, const char *text, size_t length) {
    for (size_t index = 0; index < length; ++index)
        hash = (hash ^ (unsigned char)text[index]) * FNV_PRIME;
    return hash;
}

static unsigned char dirent_type(uint32_t kind) {
    switch (kind) {
    case XILA_KIND_DIRECTORY:
        return DT_DIR;
    case XILA_KIND_BLOCK_DEVICE:
        return DT_BLK;
    case XILA_KIND_CHARACTER_DEVICE:
        return DT_CHR;
    case XILA_KIND_PIPE:
        return DT_FIFO;
    case XILA_KIND_SOCKET:
        return DT_SOCK;
    case XILA_KIND_SYMLINK:
        return DT_LNK;
    default:
        return DT_REG;
    }
}

DIR *opendir(const char *path) {
    char buffer[XILA_PATH_MAX];
    struct xila_path resolved;
    if (xila_path_resolve(AT_FDCWD, path, buffer, &resolved) != 0)
        return NULL;
    struct xila_dir *stream = xila_heap_alloc(sizeof(*stream));
    if (stream == NULL)
        return NULL;
    stream->seed = fnv(FNV_OFFSET_BASIS, buffer, strlen(buffer));
    stream->seed = fnv(stream->seed, "/", 1);
    XilaStatus status = xila_directory_iterator_open(resolved.directory, resolved.relative,
                                                     resolved.length, &stream->iterator);
    if (status != XILA_STATUS_OK) {
        xila_heap_release(stream);
        xila_fail(status);
        return NULL;
    }
    return (DIR *)stream;
}

struct dirent *readdir(DIR *directory) {
    struct xila_dir *stream = (struct xila_dir *)directory;
    size_t length = 0;
    uint32_t kind = 0;
    XilaStatus status = xila_directory_iterator_next(
        stream->iterator, stream->entry.d_name, sizeof(stream->entry.d_name) - 1, &length, &kind);
    if (status == XILA_STATUS_END)
        return NULL; /* end of stream leaves errno untouched */
    if (status == XILA_STATUS_BUFFER_TOO_SMALL) {
        errno = ENAMETOOLONG;
        return NULL;
    }
    if (status != XILA_STATUS_OK) {
        xila_fail(status);
        return NULL;
    }
    stream->entry.d_name[length] = '\0';
    stream->entry.d_type = dirent_type(kind);
    uint64_t inode = fnv(stream->seed, stream->entry.d_name, length);
    stream->entry.d_ino = (ino_t)(inode != 0 ? inode : 1);
    return &stream->entry;
}

void rewinddir(DIR *directory) {
    (void)xila_directory_iterator_rewind(((struct xila_dir *)directory)->iterator);
}

int closedir(DIR *directory) {
    struct xila_dir *stream = (struct xila_dir *)directory;
    XilaStatus status = xila_directory_iterator_close(stream->iterator);
    xila_heap_release(stream);
    return status == XILA_STATUS_OK ? 0 : xila_fail(status);
}
