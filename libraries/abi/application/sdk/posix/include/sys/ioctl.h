#ifndef _SYS_IOCTL_H_
#define _SYS_IOCTL_H_

#ifdef __cplusplus
extern "C" {
#endif

/* int * output: queued bytes on either end of a pipe. */
#define FIONREAD 0x541B

/* Retain picolibc's command numbers. The SDK returns ENOTTY for these. */
#define TIOCGWINSZ (('T' << 8) | 1)
#define TIOCSWINSZ (('T' << 8) | 2)
#define TIOCLINUX  (('T' << 8) | 3)

int ioctl(int fd, unsigned long request, void *argument);

#ifdef __cplusplus
}
#endif

#endif
