/* Address text conversion: inet_pton, inet_ntop and inet_addr. */
#include <arpa/inet.h>
#include <stdio.h>
#include <string.h>

#include "internal.h"

const struct in6_addr in6addr_any = IN6ADDR_ANY_INIT;
const struct in6_addr in6addr_loopback = IN6ADDR_LOOPBACK_INIT;

/* Dotted decimal with exactly four parts, each 0-255 without leading zeros. */
static int parse_ipv4(const char *text, size_t length, uint8_t output[4]) {
    size_t part = 0, digits = 0;
    unsigned value = 0;
    for (size_t index = 0; index <= length; ++index) {
        char character = index < length ? text[index] : '.';
        if (character >= '0' && character <= '9') {
            if (digits == 1 && value == 0)
                return 0;
            value = value * 10 + (unsigned)(character - '0');
            if (++digits > 3 || value > 255)
                return 0;
        } else if (character == '.') {
            if (digits == 0 || part == 4)
                return 0;
            output[part++] = (uint8_t)value;
            value = 0;
            digits = 0;
        } else {
            return 0;
        }
    }
    return part == 4;
}

static int hexadecimal(char character) {
    if (character >= '0' && character <= '9')
        return character - '0';
    if (character >= 'a' && character <= 'f')
        return character - 'a' + 10;
    if (character >= 'A' && character <= 'F')
        return character - 'A' + 10;
    return -1;
}

/* RFC 4291 text: eight groups, one "::" and an optional dotted IPv4 tail. */
static int parse_ipv6(const char *text, uint8_t output[16]) {
    uint8_t bytes[16] = {0};
    size_t position = 0;
    long gap = -1;
    const char *cursor = text;
    if (cursor[0] == ':') {
        if (cursor[1] != ':')
            return 0;
        gap = 0;
        cursor += 2;
    }
    while (*cursor != '\0') {
        const char *start = cursor;
        unsigned value = 0;
        int digits = 0;
        while (hexadecimal(*cursor) >= 0) {
            value = value * 16 + (unsigned)hexadecimal(*cursor++);
            if (++digits > 4)
                return 0;
        }
        if (*cursor == '.') {
            if (position > 12 || !parse_ipv4(start, strlen(start), bytes + position))
                return 0;
            position += 4;
            break;
        }
        if (digits == 0 || position == 16)
            return 0;
        bytes[position++] = (uint8_t)(value >> 8);
        bytes[position++] = (uint8_t)value;
        if (*cursor == '\0')
            break;
        if (*cursor++ != ':')
            return 0;
        if (*cursor == ':') {
            if (gap >= 0)
                return 0;
            gap = (long)position;
            if (*++cursor == '\0')
                break;
        } else if (*cursor == '\0') {
            return 0;
        }
    }
    if (gap >= 0) {
        if (position == 16)
            return 0;
        size_t tail = position - (size_t)gap;
        memmove(bytes + 16 - tail, bytes + gap, tail);
        memset(bytes + gap, 0, 16 - (size_t)gap - tail);
    } else if (position != 16) {
        return 0;
    }
    memcpy(output, bytes, 16);
    return 1;
}

int inet_pton(int family, const char *restrict text, void *restrict address) {
    if (family == AF_INET)
        return parse_ipv4(text, strlen(text), address);
    if (family == AF_INET6)
        return parse_ipv6(text, address);
    errno = EAFNOSUPPORT;
    return -1;
}

in_addr_t inet_addr(const char *text) {
    struct in_addr address;
    return inet_pton(AF_INET, text, &address) == 1 ? address.s_addr : INADDR_NONE;
}

const char *inet_ntop(int family, const void *restrict address, char *restrict text,
                      socklen_t size) {
    char buffer[INET6_ADDRSTRLEN];
    const uint8_t *bytes = address;
    if (family == AF_INET) {
        snprintf(buffer, sizeof(buffer), "%u.%u.%u.%u", bytes[0], bytes[1], bytes[2], bytes[3]);
    } else if (family == AF_INET6) {
        static const uint8_t mapped[12] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff};
        if (memcmp(bytes, mapped, sizeof(mapped)) == 0) {
            snprintf(buffer, sizeof(buffer), "::ffff:%u.%u.%u.%u", bytes[12], bytes[13],
                     bytes[14], bytes[15]);
        } else {
            /* The longest run of two or more zero groups becomes "::". */
            int best = -1, best_length = 1;
            for (int group = 0; group < 8;) {
                int length = 0;
                while (group + length < 8 && bytes[2 * (group + length)] == 0 &&
                       bytes[2 * (group + length) + 1] == 0)
                    ++length;
                if (length > best_length) {
                    best = group;
                    best_length = length;
                }
                group += length ? length : 1;
            }
            char *cursor = buffer;
            for (int group = 0; group < 8; ++group) {
                if (group == best) {
                    *cursor++ = ':';
                    if (group + best_length == 8)
                        *cursor++ = ':';
                    group += best_length - 1;
                    continue;
                }
                if (group != 0)
                    *cursor++ = ':';
                cursor += sprintf(cursor, "%x", (unsigned)(bytes[2 * group] << 8 | bytes[2 * group + 1]));
            }
            *cursor = '\0';
        }
    } else {
        errno = EAFNOSUPPORT;
        return NULL;
    }
    size_t length = strlen(buffer);
    if (length >= size) {
        errno = ENOSPC;
        return NULL;
    }
    memcpy(text, buffer, length + 1);
    return text;
}
