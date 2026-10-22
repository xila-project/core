# Command-line shell

The shell runs built-in commands and native or WebAssembly executables through Xila's task and virtual filesystem APIs. Its parser supports `no_std` and introduces no additional dependencies.

## Composing commands

```sh
export NAME='hello world'; echo "$NAME"
echo first > /temporary/example; echo second >> /temporary/example
cat < /temporary/example | wc -l
missing-command 2> /temporary/errors || echo recovered
long-running-command > /temporary/result & echo foreground
```

`|` binds more tightly than `&&` and `||`. The conditional operators have equal precedence and evaluate left to right. `;` separates sequential command lists; `&` starts the preceding conditional list in a background task. Operators do not need surrounding spaces. Redirections belong to a single command, are applied left to right, and override its pipeline endpoint when they target the same stream.

- `< path` reads standard input from a file.
- `> path` creates or truncates standard output; `>> path` appends.
- `2> path` and `2>> path` redirect standard error.
- `cat` and `wc` consume standard input when no file is given, or when a file argument is `-`. Other built-ins retain their existing input interfaces.

A zero exit status means success. Built-in failures return 1, unknown commands return 127, and malformed source passed to `shell -c` returns 2. External executable statuses are preserved within 0–255. A pipeline returns its last stage's status and waits for every stage. `true` and `false` provide status 0 and 1. `$?` expands to the preceding foreground status; successfully launching a background list sets it to zero.

Foreground built-ins can change the shell's current directory and environment. Pipeline stages and background lists run in child tasks with inherited environments and isolated shell state. Background input is `/devices/null`; output and error inherit the shell's streams unless redirected and may interleave with foreground output. Completed background tasks are reaped as new jobs start. Shell exit waits for outstanding jobs; job listing, cancellation and foreground/background switching are separate work.

## Arguments and expansion

Single quotes preserve their contents literally. Double quotes allow variable expansion. A backslash outside single quotes protects the following character; inside double quotes it protects `$`, `"` and `\`. A `#` at a word boundary starts a comment.

`$NAME`, `${NAME}` and `$?` expand when a command is prepared for execution. Names use ASCII letters, digits and underscores, with a letter or underscore first. Missing variables expand to an empty string. Expansion retains one argument per source word, including empty arguments: it does not split variable values on whitespace or interpret their contents as shell operators. Quoted and unquoted fragments can form one word.

Calling the executable with already separated arguments preserves those arguments literally. Use `shell -c 'COMMAND'` explicitly to parse shell source. Interactive input is parsed one line at a time; EOF also executes a final unterminated line.

This is a bounded shell language, not a complete POSIX shell. Scripting files, multiline syntax, substitutions, globbing, subshells, descriptor duplication such as `2>&1`, here-documents and shell assignment syntax are outside this change. Newline characters inside explicit command source currently count as whitespace; use `;` between commands.

## Memory and execution limits

| Resource                                             |      Limit |
| ---------------------------------------------------- | ---------: |
| Source line                                          | 4096 bytes |
| Tokens per line                                      |        256 |
| Commands per line                                    |         32 |
| Stages per pipeline                                  |          8 |
| Expanded arguments and redirection paths per command | 8192 bytes |
| Buffered formatted output per built-in               | 8192 bytes |
| Buffered bytes per pipe                              |        512 |
| Active background lists per shell                    |          8 |

The lexer borrows word slices and the parser validates one token vector without allocating a syntax tree. Execution allocates expanded words and task state only for the commands being prepared. Pipelines use concurrent tasks and bounded pipes with asynchronous backpressure; they do not collect complete command output in memory. Plain byte output streams directly. These limits bound shell-owned syntax and pipe storage, not allocations performed by individual commands, inherited environment values, executable runtimes or task infrastructure. Nested shell invocations have their own limits.

Duplicated file streams share their position and LittleFS context. Position storage becomes shared only on the first duplication, and duplicated streams reuse the existing LittleFS native cache rather than copying its pointers.

## Command discovery and utilities

Use `help` to list built-ins and executable entries in the task's `Paths` search
locations, or `help COMMAND` for built-in usage. Help does not launch commands.
External commands supply their own usage; names present in multiple search
locations may appear more than once. Built-in names and help use one registry.

Examples:

```text
sleep 2
sleep 250ms
chmod 0640 notes.txt
tree -L 2 /configuration
find /configuration --name settings.toml --type f
realpath ../notes.txt /configuration
help chmod
```

`realpath PATH...` prints canonical absolute paths for existing files and
directories. Relative operands are resolved from the current directory; VFS
metadata checks report missing paths and access errors.

`sleep` accepts one nonnegative integer with `ms`, `s`, `m`, or `h`; omitted
units mean seconds. Fractions, multiple durations, and values exceeding the
underlying timer's u64 nanosecond range are rejected. Waiting yields to other
tasks.

`chmod MODE PATH` accepts octal read/write/execute permissions from `000` through
`777`, optionally with a leading zero. It uses VFS ownership checks. Symbolic
modes, special bits, and recursive changes are unsupported.

`tree` includes hidden entries, skips `.` and `..`, and streams entries in
filesystem order. `-L N` limits levels below the selected directory; `-L 0`
prints only that directory. Default and maximum depth are 32, with paths bounded
to 1024 bytes. Traversal stops and reports filesystem or output errors. It keeps
one directory handle per active level rather than collecting sibling entries.

`find [PATH]` prints matching paths, starting at `PATH` or the current directory.
It includes the starting path when it matches, searches hidden entries, and
skips `.` and `..`. `-n NAME` or `--name NAME` matches an exact entry name;
`-t TYPE` or `--type TYPE` filters by `f` (file), `d` (directory), `c`
(character device), `b` (block device), `p` (pipe), `s` (socket), or `l`
(symbolic link). `-d N` or `--max-depth N` limits levels below the starting
path; the default and maximum are 32. Paths are bounded to 1024 bytes. Missing
or inaccessible paths and errors while reading a directory stop the command
with an error. Traversal streams results and retains at most one directory
handle per active level.

`mv SOURCE DESTINATION` renames a file or directory to the exact destination
path. Relative paths use the current directory. Both paths must belong to the
same mounted filesystem; the command reports the filesystem's rename error if
the source or destination cannot be renamed.
