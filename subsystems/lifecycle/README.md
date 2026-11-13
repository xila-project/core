# System lifecycle

`xila-lifecycle` implements shutdown, restart and sleep as an operating-system
mechanism. Shutdown is not a device: privileged executables request it through
this crate's Rust API, and the platform runs the pipeline. Only the final
hardware action goes through a device, the system power device
`/devices/power` (contract `device::power`, trait
`peripherals::power::PowerController`, VFS adapter
`drivers_peripherals::power::PowerHandle`, target implementations in
Platforms). The crate is `no_std` and depends on no graphics stack, shell or
particular filesystem. Battery and supply status is a separate concern
(core#147).

## Requesting

`lifecycle::request(virtual_file_system, task, action, status)` requests
`PowerOff`, `Restart` or `Sleep` on behalf of `task`:

- **Privilege**: the task's user must be allowed to open `/devices/power` for
  writing. The VFS permissions on that node are the policy; hosted platforms
  make it root and administrator only (`0660`, group `administrator`).
  Rejected callers get `PermissionDenied`.
- **Capabilities**: actions the power device does not perform fail with
  `Unsupported` before the request is accepted.
- **One winner**: a second request gets `Busy`.

WebAssembly guests have no access to this API, so they cannot request a
shutdown. `SYSTEM.request(action, status)` is the trusted entry point for
platform code: one compare-and-exchange on an atomic word, safe in a POSIX
signal handler (no allocation, lock or wakeup). The `status` byte is the
process exit status on hosted targets.

## State machine

`Controller` keeps phase, winning action and status in one atomic word.
Phases only move forward:

```text
Running → Requested → Quiescing → Syncing → StoppingDrivers → Finalizing → Returned
                          └──────────┴─────────────┴──────────────┴──→ Failed
```

`Returned` and `Failed` are terminal. An accepted request is never confused
with completion, and a failed shutdown cannot restart behind the user's back.
`executable::execute` refuses new executables once the state left `Running`.

## Pipeline

The platform's supervisor task observes `SYSTEM` and calls `Supervisor::run`.
It first opens `/devices/power` (so the final action does not depend on path
resolution after the root filesystem is unmounted) and reads its
capabilities, then:

1. **Quiesce**: `tasks::stop` sends `Signal::Termination` to every descendant
   of the supervisor task except the runtime services it keeps, tracks
   children that outlive their parent, and waits on the monotonic clock until
   they are gone or the budget elapsed (`Timeout`). Embassy cannot preempt or
   kill a task: an unresponsive task keeps running with its resources, nothing
   is freed underneath it.
2. **Storage**: `VirtualFileSystem::shutdown_storage` refuses while any file
   or directory is open, then freezes writes for good and unmounts deepest
   mounts first, root last. LittleFS syncs its backing device before
   `lfs_unmount`; host file drives use `sync_all`.
3. **Drivers**: the platform's `Drivers::stop` hook, without touching storage.
4. **Finalize**: `POWER_OFF`, `RESTART` or `SLEEP` on the power device. A
   successful action may not return (process exit, reset).

Any stage error moves to `Failed` and stops the pipeline: a failed commit never
reaches the power device. `Controller::run_forced` is the separate, dangerous
path that continues to the final action after a failure; nothing reaches it
through an executable or device.

## Limits

- Tasks and guests stop only cooperatively. Without a task signal wakeup they
  poll their termination signal at a bounded interval while blocked.
- Runtime services (graphics, network, audio) are not joined by the hosted
  supervisors yet; they keep their memory until the process or instance ends.
  Implement their stop hooks before using this pipeline to remove board power.
- No guarantee is made for power loss or brownout: that needs power-safe
  storage, not a graceful teardown.
- `VirtualFileSystem::uninitialize` and `unmount_all` are not shutdown entry
  points.
