# System activity (native)

System activity is a native Rust Xila application, mounted at
`/applications/executables/activity` by the native example. It uses
`ExecutableTrait` and Xila graphics directly; no WebAssembly module is built or
loaded for it. A fresh native seed includes its desktop shortcut. Existing disks
can launch it from Terminal with `/applications/executables/activity`, or regenerate a separate seed/disk.

Launch with `/applications/executables/activity --interval-ms 250`. The default refresh is 1000 ms; accepted
values are 100–10000 ms. The Refresh button cycles 250, 1000 and 2000 ms. Sampling
and event waits use async timers; provider reads happen outside the graphics
lock. Window events do not reset the sampling deadline.

## Metric sources and limits

| Metric | Source | Meaning |
| --- | --- | --- |
| Uptime | `ExecutableContext.time_manager.uptime()` | Elapsed monotonic time since Xila time manager initialization, unaffected by UTC adjustments |
| Registered tasks | `task::Manager::get_task_count()` | Current registry entries, including root and runtime tasks; no tree walk or allocation |
| Allocator used bytes | `memory::global_manager().get_used()` | Xila allocator usage, not process RSS or per-app memory |
| Allocator capacity | `memory::global_manager().get_total_size()` | Reported allocator capacity; unavailable when zero or inconsistent |
| CPU load, registered devices | No suitable provider | Explicitly unavailable |

Samples are observations taken in sequence, not an atomic system snapshot.
Native allocation regions may be retained after freeing, so capacity can grow
without shrinking. This is a demo dashboard, not a hardware memory/CPU profiler.
New metrics should read subsystem providers in the sampling loop, preserve an
explicit unavailable state, and document their scope here.

## Bounded workload

Start demo retains a fallibly allocated 4096-byte buffer inside the app. Its
async loop mutates at most 512 bytes per 25 ms batch and stops automatically
after 10 seconds. Missed batches are skipped rather than replayed. Stop, window
close and app cancellation immediately drop the owned buffer; no demo task or
executor pool is spawned. Repeated starts cannot create concurrent workloads in
one window. Allocation failure is shown without taking down the dashboard. The
extra buffer makes memory usage observable without a busy loop or privileges.

## Native smoke test

Build a fresh native image (`mise run image:native`) and run:

```sh
cargo run -p xila-native-example --bin native_example -- \
  --display-driver headless --resolution 800x600 --no-network \
  --seed-image target/seed/native/seed.img --disk-path /tmp/activity-demo.img
```

Use a new disk path to avoid modifying an existing installation. Login as the
seeded `administrator` (empty password), open System activity on the desktop.
Use the Debug client as the same OS user:

```sh
cargo run -p xila-drivers-native --bin xila-debug -- ui-tree
cargo run -p xila-drivers-native --bin xila-debug -- diagnostics
cargo run -p xila-drivers-native --bin xila-debug -- screenshot /tmp/activity.png
```

Check increasing uptime across two refreshes, nonzero task and allocator values,
and explicit unavailable metrics. Change Refresh and observe continued updates.
Start/stop/restart the demo; allocator usage should rise while running and
return near its previous value after Stop or the 10-second limit. Registered
task count remains unchanged because the work runs inside the app task. Close
while running and confirm the dashboard task disappears while the shell remains
responsive. Use the bottom home gesture, then long-press the app dock icon and
choose Close. Pointer/key commands are documented in the native example README.

Automated workload/configuration and registry tests:
`cargo test -p xila-activity --lib` and
`cargo test -p xila-task --lib task_count_tracks_registration_and_completion`.

### Observed Linux smoke result

On the headless native target at 800×600 with networking disabled and a fresh
seeded disk, uptime advanced from 105 to 107 seconds while open. Registered task
count stayed at six during the in-app demo. Allocator usage changed from 625464
to 629568 bytes while running and returned to 625464 after Stop; automatic stop
also returned to that baseline. A separate launch with `--interval-ms 10000`
processed rapid Start/Stop within 0.65 s and closed while running within 0.33 s
(measured host-client round trips, not a latency guarantee). No MCU hardware or
non-Linux platform claim is made by this smoke test.
