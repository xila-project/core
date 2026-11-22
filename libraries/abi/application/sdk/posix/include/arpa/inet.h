/* Byte order and address text conversion. Replaces picolibc's header, which only has the
 * byte order macros. */
#ifndef _XILA_ARPA_INET_H
#define _XILA_ARPA_INET_H

#include <netinet/in.h>

#ifdef __cplusplus
extern "C" {
#endif

int inet_pton(int family, const char *restrict text, void *restrict address);
const char *inet_ntop(int family, const void *restrict address, char *restrict text,
                      socklen_t size);
in_addr_t inet_addr(const char *text);

#ifdef __cplusplus
}
#endif

#endif
