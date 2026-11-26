/* hexdump for Xila native applications, built against the SDK with CMake.
 *
 * Canonical hex + printable ASCII dump of files (or of stdin), and the reverse
 * operation (-r): hex text back to raw bytes. Standard POSIX C11 only (getopt,
 * stdio, strerror(errno), malloc), no mutable static globals of its own, no
 * fork/mmap, and at most a few hundred bytes of stack: the row and discard
 * buffers are 64 bytes each and the -r line buffer lives on the heap.
 *
 * Build with the SDK's toolchain file:
 *   cmake -S . -B build -DCMAKE_TOOLCHAIN_FILE=<sdk>/cmake/xila-toolchain.cmake
 *   cmake --build build --target hexdump     # build/hexdump.elf
 */
#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define DEFAULT_COLUMNS 16UL
#define MAX_COLUMNS 64UL

static void print_usage(FILE *stream, const char *program) {
    fprintf(stream,
            "usage: %s [-r] [-n length] [-s skip] [-c columns] [file ...]\n"
            "Dump each file (stdin when no file is given; \"-\" also means stdin) as a\n"
            "canonical hex + ASCII listing. The inputs form one concatenated stream:\n"
            "the offset keeps counting across files, -s skips once at the start and\n"
            "-n limits the total.\n"
            "\n"
            "Each output line is:  %%08lx  hex bytes  |ascii|\\n\n"
            "  the input offset in hex (at least 8 digits), two spaces, the row's\n"
            "  bytes in hex separated by single spaces (one extra space after the\n"
            "  8th byte when -c is 16, and partial final rows padded with spaces so\n"
            "  the ASCII column stays aligned), two spaces, then the row's bytes\n"
            "  that are printable (0x20..0x7e) or '.' otherwise, between pipes.\n"
            "\n"
            "  -n length    dump at most length bytes (non-negative decimal integer);\n"
            "               with -r, write at most length raw bytes\n"
            "  -s skip      skip skip bytes of input first: fseek on seekable files, a\n"
            "               discard-read loop on non-seekable streams (pipes, stdin)\n"
            "  -c columns   bytes per line, 1..64 (default 16)\n"
            "  -r           reverse: parse hex input produced by this tool or raw\n"
            "               whitespace-separated hex tokens (not util-linux '*' repeat\n"
            "               markers or standalone trailing offset lines) and write the\n"
            "               raw bytes to stdout: a leading token of 8 or more hex\n"
            "               digits followed by more tokens on the line is an offset\n"
            "               and is skipped, and everything from the first '|' on is\n"
            "               the ASCII column and is ignored\n"
            "  -h           print this help on stdout and exit 0\n"
            "\n"
            "Exit status: 0 on success, 1 on an I/O or file-open error (\"<program>:\n"
            "<path>: <strerror(errno)>\" on stderr), 2 on an invalid option or\n"
            "argument (this usage on stderr).\n",
            program);
}

static int usage_error(const char *program) {
    print_usage(stderr, program);
    return 2;
}

/* "<program>: <path>: <strerror(errno)>" on stderr; the exit status of an I/O error. */
static int report_io(const char *program, const char *path) {
    fprintf(stderr, "%s: %s: %s\n", program, path, strerror(errno));
    return 1;
}

static int report_stdout_io(const char *program) {
    if (errno == 0)
        errno = EIO;
    return report_io(program, "stdout");
}

static int hex_value(int c) {
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
    return -1;
}

/* A non-negative decimal integer: digits only, no sign, no trailing junk. */
static int parse_count(const char *text, unsigned long *value) {
    if (!isdigit((unsigned char)text[0]))
        return -1;
    for (const char *p = text + 1; *p != '\0'; p++)
        if (!isdigit((unsigned char)*p))
            return -1;
    errno = 0;
    char *end = NULL;
    unsigned long parsed = strtoul(text, &end, 10);
    if (errno == ERANGE || end == NULL || *end != '\0')
        return -1;
    *value = parsed;
    return 0;
}

/* Skip up to *skip bytes of stream, decrementing *skip and advancing *offset
 * by the actual number of bytes skipped. Uses ftell/fseek on seekable files so
 * a skip larger than the current file carries the remainder into the next file,
 * and falls back to a discard-read loop on non-seekable streams. */
static int skip_bytes(FILE *stream, unsigned long *skip, unsigned long *offset,
                      const char *program, const char *path) {
    long cur = ftell(stream);
    if (cur >= 0 && fseek(stream, 0, SEEK_END) == 0) {
        long end = ftell(stream);
        if (end >= cur) {
            unsigned long available = (unsigned long)(end - cur);
            unsigned long advance = *skip < available ? *skip : available;
            if (advance < available && fseek(stream, cur + (long)advance, SEEK_SET) != 0)
                return report_io(program, path);
            *skip -= advance;
            if (offset != NULL)
                *offset += advance;
            return 0;
        }
        (void)fseek(stream, cur, SEEK_SET);
    }
    /* A non-seekable stream leaves ESPIPE in errno from ftell/fseek; clear it before reading. */
    errno = 0;
    unsigned char discard[MAX_COLUMNS];
    while (*skip > 0) {
        size_t want = *skip < sizeof discard ? (size_t)*skip : sizeof discard;
        size_t got = fread(discard, 1, want, stream);
        if (got == 0) {
            if (ferror(stream))
                return report_io(program, path);
            return 0; /* EOF: remaining *skip carries over to the next file */
        }
        *skip -= got;
        if (offset != NULL)
            *offset += got;
    }
    return 0;
}

/* One canonical line: offset, hex bytes (padded to columns), then the printable ASCII column. */
static void print_row(unsigned long offset, const unsigned char *row, size_t count,
                      size_t columns) {
    printf("%08lx  ", offset);
    for (size_t i = 0; i < columns; i++) {
        if (i > 0)
            putchar(' ');
        if (i < count)
            printf("%02x", (unsigned)row[i]);
        else
            fputs("  ", stdout);
        if (columns == DEFAULT_COLUMNS && i == 7)
            putchar(' ');
    }
    fputs("  |", stdout);
    for (size_t i = 0; i < count; i++)
        putchar(row[i] >= 0x20 && row[i] <= 0x7e ? row[i] : '.');
    fputs("|\n", stdout);
}

static int dump_stream(FILE *stream, const char *path, unsigned long *skip,
                       unsigned long *limit, size_t columns, unsigned long *offset,
                       const char *program) {
    if (*skip > 0) {
        int status = skip_bytes(stream, skip, offset, program, path);
        if (status != 0)
            return status;
        if (*skip > 0)
            return 0; /* this stream ended before finishing the skip */
    }
    unsigned char row[MAX_COLUMNS];
    while (*limit > 0) {
        size_t want = columns;
        if (*limit < (unsigned long)want)
            want = (size_t)*limit;
        size_t count = fread(row, 1, want, stream);
        if (count == 0) {
            if (ferror(stream))
                return report_io(program, path);
            break; /* EOF */
        }
        print_row(*offset, row, count, columns);
        if (ferror(stdout))
            return report_stdout_io(program);
        *offset += count;
        *limit -= count;
        if (count < want)
            break; /* EOF */
    }
    return 0;
}

/* Buffered raw-byte output of -r mode. */
struct reverse_writer {
    unsigned char buffer[MAX_COLUMNS];
    size_t count;
    unsigned long limit; /* remaining bytes to write, ULONG_MAX for no limit */
    const char *program;
};

static int reverse_flush(struct reverse_writer *writer) {
    if (writer->count == 0)
        return 0;
    if (fwrite(writer->buffer, 1, writer->count, stdout) != writer->count) {
        int saved = errno;
        fprintf(stderr, "%s: stdout: %s\n", writer->program, strerror(saved));
        return -1;
    }
    writer->count = 0;
    return 0;
}

/* Queue one byte; returns 1 when the -n limit is reached (stop), -1 on error. */
static int reverse_byte(struct reverse_writer *writer, unsigned char byte) {
    if (writer->limit == 0)
        return 1;
    writer->buffer[writer->count++] = byte;
    writer->limit--;
    if (writer->count == sizeof writer->buffer)
        return reverse_flush(writer);
    return 0;
}

/* Parse one line of hex input. Everything from the first '|' on is the ASCII
 * column and is ignored. Tokens are whitespace-separated; the first token of
 * the line is an offset (skipped) when it is 8 or more hex digits followed by
 * more tokens. Returns 0 to continue, 1 when the -n limit stopped the output,
 * -1 on invalid input. */
static int parse_hex_line(char *line, struct reverse_writer *writer, const char *program,
                          const char *path) {
    char *ascii = strchr(line, '|');
    if (ascii != NULL)
        *ascii = '\0';
    char *cursor = line;
    int first = 1;
    for (;;) {
        while (isspace((unsigned char)*cursor))
            cursor++;
        if (*cursor == '\0')
            break;
        char *token = cursor;
        while (*cursor != '\0' && !isspace((unsigned char)*cursor))
            cursor++;
        size_t length = (size_t)(cursor - token);
        int hex_only = 1;
        for (size_t i = 0; i < length; i++)
            if (hex_value((unsigned char)token[i]) < 0) {
                hex_only = 0;
                break;
            }
        int is_offset = 0;
        if (first && hex_only && length >= 8) {
            char *rest = cursor;
            while (*rest == ' ' || *rest == '\t')
                rest++;
            if (*rest != '\0' && !isspace((unsigned char)*rest))
                is_offset = 1;
        }
        first = 0;
        if (is_offset)
            continue;
        if (!hex_only || (length % 2) != 0) {
            fprintf(stderr, "%s: %s: invalid hex input\n", program, path);
            return -1;
        }
        for (size_t i = 0; i < length; i += 2) {
            unsigned char byte =
                (unsigned char)((hex_value((unsigned char)token[i]) << 4) |
                                hex_value((unsigned char)token[i + 1]));
            int status = reverse_byte(writer, byte);
            if (status != 0)
                return status;
        }
    }
    return 0;
}

/* Read one line into *buffer, growing it as needed (the newline is kept).
 * Returns the length, 0 at EOF with nothing read, or -1 on error. */
static long read_line(FILE *stream, char **buffer, size_t *capacity) {
    size_t length = 0;
    for (;;) {
        if (length + 2 > *capacity) {
            size_t grown = *capacity * 2;
            char *moved = realloc(*buffer, grown);
            if (moved == NULL)
                return -1;
            *buffer = moved;
            *capacity = grown;
        }
        if (fgets(*buffer + length, (int)(*capacity - length), stream) == NULL) {
            if (ferror(stream))
                return -1;
            return (long)length; /* EOF: 0 when nothing was read */
        }
        length += strlen(*buffer + length);
        if (length > 0 && (*buffer)[length - 1] == '\n')
            return (long)length;
        if (feof(stream))
            return (long)length;
        /* the buffer filled without a newline: keep reading the same line */
    }
}

static int reverse_stream(FILE *stream, const char *path, unsigned long *skip,
                          unsigned long *limit, const char *program) {
    if (*skip > 0) {
        int status = skip_bytes(stream, skip, NULL, program, path);
        if (status != 0)
            return status;
        if (*skip > 0)
            return 0; /* this stream ended before finishing the skip */
    }
    size_t capacity = 256;
    char *line = malloc(capacity);
    if (line == NULL) {
        errno = ENOMEM;
        return report_io(program, path);
    }
    struct reverse_writer writer;
    writer.count = 0;
    writer.limit = *limit;
    writer.program = program;
    int status = 0;
    for (;;) {
        long length = read_line(stream, &line, &capacity);
        if (length < 0) {
            status = report_io(program, path);
            break;
        }
        if (length == 0)
            break; /* EOF */
        int parsed = parse_hex_line(line, &writer, program, path);
        if (parsed != 0) {
            status = parsed < 0 ? 1 : 0;
            break;
        }
    }
    if (status == 0 && reverse_flush(&writer) != 0)
        status = 1;
    free(line);
    *limit = writer.limit;
    return status;
}

static int process(FILE *stream, const char *path, int reverse, unsigned long *skip,
                   unsigned long *limit, size_t columns, unsigned long *offset,
                   const char *program) {
    if (reverse)
        return reverse_stream(stream, path, skip, limit, program);
    return dump_stream(stream, path, skip, limit, columns, offset, program);
}

int main(int argc, char **argv) {
    const char *program = argc > 0 && argv[0] != NULL ? argv[0] : "hexdump";
    int reverse = 0;
    unsigned long limit = ULONG_MAX;
    unsigned long skip = 0;
    unsigned long columns_value = DEFAULT_COLUMNS;
    opterr = 0; /* the usage diagnostic is ours, not getopt's */
    int option;
    while ((option = getopt(argc, argv, "n:s:c:rh")) != -1) {
        switch (option) {
        case 'n':
            if (parse_count(optarg, &limit) != 0)
                return usage_error(program);
            break;
        case 's':
            if (parse_count(optarg, &skip) != 0)
                return usage_error(program);
            break;
        case 'c':
            if (parse_count(optarg, &columns_value) != 0 || columns_value < 1 ||
                columns_value > MAX_COLUMNS)
                return usage_error(program);
            break;
        case 'r':
            reverse = 1;
            break;
        case 'h':
            print_usage(stdout, program);
            if (fflush(stdout) != 0 || ferror(stdout))
                return report_stdout_io(program);
            return 0;
        default:
            return usage_error(program);
        }
    }
    size_t columns = (size_t)columns_value;
    unsigned long offset = 0;
    if (optind >= argc) {
        int status = process(stdin, "-", reverse, &skip, &limit, columns, &offset, program);
        if (status != 0)
            return status;
    } else {
        for (int index = optind; index < argc; index++) {
            const char *path = argv[index];
            FILE *stream = stdin;
            if (strcmp(path, "-") != 0) {
                stream = fopen(path, "rb");
                if (stream == NULL)
                    return report_io(program, path);
            }
            int status = process(stream, path, reverse, &skip, &limit, columns, &offset, program);
            if (stream != stdin)
                fclose(stream);
            if (status != 0)
                return status;
            if (limit == 0)
                break;
        }
    }
    if (fflush(stdout) != 0 || ferror(stdout))
        return report_stdout_io(program);
    return 0;
}
