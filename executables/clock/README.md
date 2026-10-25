# Clock for Xila

A graphical Rust/Wasm Clock application using Xila's LVGL bindings and shared
Shadcn theme. This is the first increment of [#310](https://github.com/xila-project/core/issues/310).

## Available views

- **Clock:** UTC date/time from the host, with a saved 12/24-hour preference.
- **Alarm:** one saved alarm, editable hour/minute, selected weekdays or a
  one-time occurrence, enable/disable/delete, five-minute snooze, and dismiss.
- **Stopwatch:** start/pause/resume/reset and up to 32 cumulative lap times.
- **Timer:** one countdown, from one second to 24 hours, with pause/resume,
  restart/cancel, and a visible completion alert. The duration controls select
  the next countdown; Start replaces the current countdown.

Controls use buttons and checkboxes so no keyboard is required. All alarm times
are explicitly **UTC**; no fixed city offsets are presented as timezone support.
The host supplies the date/time, including whether its clock is set correctly.

## Build and run

From the repository root, in the configured development shell:

```sh
cargo build -p xila-clock --target wasm32-wasip1 --profile release-wasm
```

The native example builds, copies, and installs Clock alongside Calculator and
Weather. Run the native example using the normal graphical development workflow,
then open **Clock** from the launcher. With `prebuilt-applications`, run
`mise run build:applications` to prepare all three apps first.

The browser example also bundles Clock and installs its launcher shortcut at
boot, just like Calculator. Build both guests with `--release` before building
the browser host; see the [browser instructions](../../examples/wasm/README.md).

Other hosts can load `target/wasm32-wasip1/release-wasm/clock.wasm` into the VFS
at `/applications/executables/clock`, then run these commands in **Xila's shell**:

```sh
wasm --install /applications/executables/clock
wasm /applications/executables/clock
```

The install export creates `/configuration/shared/applications/shortcuts/clock.json`.
The launcher provisions a writable
`/users/<user-id>/applications/clock/configuration` directory for each user.

## Timing and lifecycle

Stopwatch, countdown and snooze durations use the WASI monotonic clock. They are
computed from elapsed time/deadlines, not UI refresh counts, so delayed rendering
or changing tabs does not lose time. WASI clock reads and absolute clock polls
use the same executor clock; wall-clock corrections do not move their deadlines.
Suspend behavior depends on the platform's Embassy clock driver.

Alarms use wall time. A new one-time alarm targets the next matching UTC time,
including tomorrow if today's time has passed. Repeating weekdays use
Monday through Sunday. A delayed check or restart coalesces missed occurrences
into **one** alert; it does not replay every missed day. A backward correction
waits for the already scheduled occurrence and does not replay a fired alarm.
Changing an alarm and saving it creates a new schedule. Alarm state is persisted
when an occurrence fires; if that write fails, a future launch may replay it.

Alerts are **visual and in-app**. They keep counting when another tab/window is
focused while the process runs, but a hidden window does not provide a system
notification. Closing Clock stops delivery; after relaunch, saved overdue alarms
are handled as described above. Timers, stopwatch, laps and snooze are deliberately
not restored after process exit or reboot. There is no audio alert, wake-from-sleep,
powered-off alarm, or host background alarm service in this increment.

The guest currently lacks a blocking window-event wait API. Clock cooperatively
sleeps between event checks: 250 ms normally, 100 ms while the visible stopwatch
runs. Normal display updates occur once per second; the visible stopwatch uses
tenths of a second. Only changed labels in the active view and the alert banner
are sent to LVGL. Input work per turn and retained laps are bounded. Delivery is
subject to executor scheduling; this is not a real-time alarm guarantee.

## Settings and storage

The initial settings are shared by the device, in
`/users/<user-id>/applications/clock/configuration/settings.0` and `settings.1`; use a single Clock
instance when editing settings. Each record is 38 bytes (generation, versioned
payload, checksum), and reads use fixed buffers. Writes alternate slots so a torn
or corrupt latest record can fall back to the previous valid one. Settings are
written on edits and alarm occurrences, never on every time/display update.

If neither existing record is valid or accessible, the app shows a persistence
warning, starts with alarms disabled, and leaves those records untouched.
The current WASI filesystem interface has no sync/rename support; this is
recovery from a detectable partial write, not a guarantee of durability through
power loss. Shared storage also does not serialize concurrent app instances.

## Follow-up work in #310

- Saved city/world clocks and daylight-saving-aware timezone data/APIs.
- Multiple alarms with labels, user-scoped settings, and concurrent editing.
- Host-owned scheduling/notifications for delivery after the guest exits, plus
  audio output and platform-specific RTC wake capabilities.
- Blocking guest event waits, interactive display verification, constrained-target
  validation, and measured CPU/RAM consumption of representative app workloads.

## Validation

```sh
cargo test -p xila-clock
cargo clippy -p xila-clock --all-targets --target wasm32-wasip1 -- -D warnings
cargo test -p xila-wasm --lib --features graphics,xila/graphics_rendering_xrgb8888
```

The pure timing models accept supplied timestamps for deterministic tests of
delayed checks, pause/resume, recurrence, clock corrections, and snooze. Storage
tests cover recovery from a corrupt latest record. Wasmi tests check installation
and run the real guest on a headless LVGL display: stopwatch laps, countdown
completion, saving/reloading alarms as a normal user, and window cleanup.
