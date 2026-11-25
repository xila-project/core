# xila-local-time

The system time zone setting of Xila. The crate stores the time zone that the
user chose, loads it at start-up and gives it to every task as the POSIX `TZ`
environment variable. Programs convert UTC to local time with
[`xila-time-zone`](../../libraries/time_zone/README.md).

Civil time stays outside the kernel clock: the `time` subsystem keeps UTC and
monotonic time only (xila-project/core#463). This crate does not change UTC.
When UTC is not synchronized, the programs that show local time must say so
(the time subsystem reports the status); a stored time zone does not make the
clock valid.

The crate is re-exported as `xila::local_time` with the `time_zone` feature of
the `xila` facade. It uses no IANA zone table: Core reads and publishes only
POSIX TZ strings.

## Stored setting

`/configuration/system/time_zone.json`:

```json
{ "name": "Europe/Paris", "time_zone": "CET-1CEST,M3.5.0,M10.5.0/3" }
```

| Key | Required | Meaning |
| --- | --- | --- |
| `time_zone` | yes | The POSIX TZ string, at most 64 bytes. Core reads and publishes only this value. |
| `name` | no | The IANA zone name that the user chose, at most 64 bytes of ASCII letters, digits, `/`, `_`, `-` and `+`. It is for display only and is absent for a custom TZ string. A program with a newer zone table can resolve it again. An invalid name is dropped; the TZ string still applies. |

Unknown keys are ignored. The file is at most 256 bytes.

- Only root may have written the file: it belongs to root, others cannot write
  it, and its group can only if it is root's group. Everyone can read it.
- `local_time::set` replaces it atomically: it creates
  `/configuration/system/.time_zone.json-new` with mode 0644, writes it, then
  renames it. A failed write keeps the old file and the old `TZ`. Calls of
  `set` are serialised by a mutex across the write and the publication, so the
  file and `TZ` always hold the same setting.
- A missing, unreadable, too large or invalid file, or a file that others can
  write, gives UTC (`UTC0`). `initialize` logs one warning; `load` logs
  nothing. Start-up continues.

## Start-up order

The platform calls `local_time::initialize` once, after it mounts the file
systems and creates the default hierarchy, and before it starts the shell or
the startup application. The graphical shell is not needed. `initialize`
sets `TZ` in the root task and in every task that has no `TZ` yet (the services
started before it).

## Publication and changes

- Every task inherits the environment of its parent, so a native executable
  and a WebAssembly guest (through WASI `environ_get`) see `TZ` from their
  start.
- `local_time::set` checks that the calling user is root or a member of the
  administrator group, writes the file as root, then calls
  `task::Manager::set_system_environment_variable`. That call gives `TZ` the new
  value in the root task and in every task whose `TZ` is still the previous
  system value, under one lock. A task that set a `TZ` of its own (for example
  with `export TZ=...` in the shell) keeps it. A task that exported a `TZ`
  equal to the system value cannot be told apart from one that inherited it,
  so it follows the next change.
- Tasks started after a change get the new value at once.
- A native program that reads `TZ` on each display update, for example with
  `local_time::current`, follows a change at its next update. The graphical
  shell clock does this.
- A WebAssembly guest copies its environment when it starts. A running guest
  keeps the old value until it is restarted.

## API

| Item | Use |
| --- | --- |
| `Setting::new(name, time_zone)` | Check and build a setting. |
| `validate_time_zone(text)` | Check a TZ string (length and syntax), for input fields. |
| `read(vfs, task)` | The stored setting, or the reason it cannot be used. |
| `load(vfs, task)` | The stored setting, or UTC with one warning. |
| `initialize(task_manager, vfs, task)` | `load`, then publish `TZ`. Start-up only. |
| `set(task_manager, users_manager, vfs, task, &setting)` | Store and publish a new setting. |
| `current(task_manager, task)` | The parsed `TZ` of a task, or UTC. |
| `write_offset(output, seconds)` | Write a UTC offset as `+HH:MM`. |
| `write_local_time(output, zone, utc)` | Write a local time, such as `2026-03-29 03:00:00 CEST (UTC+02:00)`. Settings and the shell share it (a `dyn` writer, linked once). |

On 32-bit targets, `Setting` is 24 bytes (two heap strings) and `SettingError`
is 1 byte. `read` allocates a 257 byte buffer on the heap for the duration of
the read.

## Tests

```
cargo test -p xila-local-time
cargo test -p xila-task --lib system_environment
```

The tests cover the start-up with no file, with an IANA name, with a custom TZ
string, with corrupt files and with a file that others can write; the
inheritance of `TZ` by a new task; `set` as root, as a member of the
administrator group and as another user; a task with its own `TZ`; a failed
write; and two concurrent `set` calls, after which the file and `TZ` agree.
