# Shutdown and power management

Shutdown is an OS lifecycle operation, not the same as turning off the host
computer, powering down an MCU, or putting it to sleep.

## Initial interface

- `/devices/power/control` is a root-owned **root-only** character device.
- `GET_SHUTDOWN_STATE` (read ioctl): 0 Running, 1 Requested, 2 Stopping,
  3 Finalizing, 4 Stopped, 5 Failed.
- `REQUEST_SHUTDOWN` (write ioctl): request shutdown atomically, returning
  busy if shutdown was already requested.
- The shell's `shutdown` builtin sends this ioctl. `exit` only ends a shell.
- The driver intentionally does not expose arbitrary writes, reboot, suspend,
  power-off or an unprivileged power interface.

The command is supported in both native runners and the browser example;
those hosts must install the device with USER_READ_WRITE permissions, **not**
the broad DEVICE_DEFAULT permissions.

## Backend behavior

- Native: exit the foreground shell; leave the native Embassy executor through
  process termination. It does **not** power off the actual computer.
- Headless xila-runtime: host SIGINT/SIGTERM atomically request shutdown and
  continue to interrupt WASM work; the signal exit status remains 128 + signal.
- Browser: exit the graphical shell; stop and join the OPFS autosave task, stop
  writes, commit the last disk snapshot and release the disk lease. No tab close.
- ESP32 and other microcontrollers: **not yet wired up**. A board-specific
  callback is needed for reset, power-latch release and deep-sleep/wakeup.

## Important: incomplete teardown barrier

This PR establishes control-plane plumbing and basic hosted flow, **not**
a guarantee of power-loss-safe OS shutdown. There is no system-wide task
supervisor or cancellation/join protocol yet, and current
`VirtualFileSystem::uninitialize` only drops mount references instead of
syncing and unmounting all mounted filesystems. Background tasks and opened
file handles can still exist when a shell exits. Do not enable actual hardware
power-off using this state alone.

Next steps:
1. Request termination of all guests/services, then join them within bounded
   monotonic timeouts. Never free a VFS referenced by live tasks.
2. Freeze new writes, close remaining files, call filesystem-specific
   flush/unmount (especially LittleFS `lfs_unmount`), and sync host/block
   storage. Report failure rather than silently marking success.
3. Add backend capability discovery, hardware poweroff/restart and
   suspend/deep sleep only when supported; define wake sources and resume.
4. Add integration tests for concurrent writers, storage errors, POSIX signals,
   VFS authorization, background guest cancellation and browser OPFS snapshots.
5. Keep battery/power-supply reporting (#147) independent of lifecycle control.
