# Device polling through the VFS

This is the first migration step for #252. It supplies an internal polling
contract and migrates pipes. It does not implement the descriptor readiness ABI
or close the remaining retry work in #252.

## Device contract

`BaseOperations` remains object-safe. A device opts in with
`supports_polling() == true` and overrides the operations it needs:

```rust,ignore
fn poll_read(
    &self,
    opening: &mut file_system::Context,
    task: &mut core::task::Context<'_>,
    buffer: &mut [u8],
    position: file_system::Size,
) -> core::task::Poll<file_system::Result<usize>>;
```

The equivalent write, flush and control hooks use the existing synchronous
arguments plus the task context. `DirectBaseOperations` exposes the same hooks
without an opening context, and its blanket adapter forwards them. A device
requiring per-opening state implements `BaseOperations` directly.

There are two independent contexts:

- `file_system::Context` holds device-defined private data. Initialize it during
  open, using the existing type-erased storage. Duplicating an opening calls
  `clone_context`; registrations must be independent, even for identical wakers.
- `core::task::Context` supplies the current task's waker. It is borrowed for one
  poll call. Clone its waker if the device needs a later notification.

No device trait requires `async fn`, an associated future, or a boxed future.
The public async `File` adapters use concrete borrowed futures on the caller's
task. They do not spawn tasks or introduce an executor.

### Ready, pending and progress

Return `Ready(Ok(count))` for successful I/O, including partial progress, and
`Ready(Err(error))` for errors. Defaults call the synchronous operation and
return its result immediately. **A busy error does not imply `Pending`.**

Return `Pending` only after arranging an event-driven wakeup. Check readiness
and register atomically under a short lock, or use check/register/recheck.
Wake means recheck, not ownership of the available data or space. Do not retain
references or pointers to the caller's buffer, control payload, or task context
between poll calls. Pending read/write must not consume bytes without reporting
progress. Pending control may start device-owned hardware work, but subsequent
polls must not submit duplicate work.

Never call `wake()` to simulate a yield. Release device locks before cloning,
dropping, or waking externally supplied wakers. These operations may execute
arbitrary waker code. Device-specific event queues, overflow policy, hardware
work and readiness state remain the device's responsibility.

### Cancellation and lifecycle

`File` mutably borrows one opening for an operation, so that opening has at most
one active polling registration. A guard calls
`cancel_operation(opening, Operation)` on completion, error and future drop.
The hook is idempotent and detaches registrations; it need not undo hardware
work. An external timeout cancels the registration by dropping the future.
There is no internal timeout service.

Close must also remove registrations. Device removal must notify waiters, then
make later polls return an appropriate terminal result. Cancellation and close
must not leave a registration referencing a reused opening. A driver that keeps
hardware work running after cancellation must keep its own valid storage.

## VFS policy

Nonblocking primitive read/write/flush/control attempts the synchronous operation
once, without registering a waker or retrying. Polling devices use their polling
hooks for blocking async operations. Non-polling devices retain the existing
cooperative busy-retry path during migration. No operation enters both paths.

The cursor commits successful read/write counts only. A pending operation does
not retain the shared cursor lock. Concurrent contention on a duplicated cursor
returns VFS `RessourceBusy` directly on the new path; it does not manufacture a
pending I/O wait or self-wake loop. Synchronous file methods keep their existing
single-attempt behavior.

Migrated `write_line` and `read_to_end` preserve progress across waits.
`read_until` waits for the first byte, then returns available progress when a
further synchronous read would block, preserving short-read behavior. It checks
the whole accumulated suffix for the delimiter. `read_to_end` rejects a zero
chunk size. Nonblocking compound helpers return successful partial progress
when they subsequently encounter busy. Cancelling `read_to_end` keeps already
appended bytes in the caller's vector and their committed cursor position;
cancelling a write does not undo bytes already sent.

## Pipe implementation and bounds

One pipe has **16 opening slots**, including reader/writer duplicates. Each slot
owns an independent optional waker. Opening or duplicating when all slots are
occupied returns `file_system::Error::TooManyOpenFiles`. Closing frees the slot;
cancelling an operation clears its waker but leaves the opening reserved.

The byte queue allocates its configured capacity at pipe initialization. Each
opening allocates its private context once. There are no waiter-node, future or
notification allocations on the steady-state path. The slots use a fixed array;
a notification uses a temporary fixed array of up to 16 wakers on the stack.
Memory and critical-section copy time therefore depend on the configured byte
capacity, in addition to this fixed per-pipe and stack overhead. Choose pipe
capacity with the target's RAM and interrupt-latency budget in mind.

Readiness checks, queue operations, registrations and endpoint counts share a
critical-section lock. Replaced and notified wakers leave the lock before drop
or wake. Writes notify pending readers; reads notify pending writers. Last-writer
close wakes readers, which consume buffered bytes before EOF. Last-reader close
wakes writers, which report the existing `InputOutput` error. Empty I/O completes
immediately. Waking all current waiters uses bounded storage and clears their
registrations; re-polling losers registers them again.

Native tests exercise actual VFS pipes, duplicate openings, registration ordering,
waker replacement, cancellation/deadline composition, EOF, endpoint close,
capacity exhaustion/reuse and progress in compound helpers. A reentrant waker
checks wake/drop lock discipline. An allocator window, limited to the test's
thread and excluding setup/open/duplicate, asserts zero Rust allocator requests
across 128 poll/wake/cancel cycles with a shared cursor. This does not measure
MCU ISR latency, arbitrary custom waker implementations, or hardware backends.

## Remaining retry inventory

| Current location | Waiting/retry owner and migration work |
| --- | --- |
| `File` primitive I/O on polling devices | Device owns wakeup registration; VFS owns blocking policy and cleanup. No generic busy retry. |
| `File` primitive I/O on legacy devices, including terminal input | Existing VFS `poll` cooperatively retries. Migrate each backend with a real wakeup source before opting in. |
| `File` metadata, seek and duplicate; VFS lookup/open/create/remove/close and attribute utilities | Existing VFS cooperative retry remains. Resource contention needs an explicit owner/wakeup design separate from I/O readiness. |
| `Directory` and `SynchronousDirectory` | Existing VFS cooperative helper / synchronous `blocking_operation` remains; the synchronous helper still spins on busy. |
| LittleFS `read_callback`, `programm_callback`, `erase_callback`, `flush_callback` | Synchronous C callbacks still retry busy locally. A separately designed blocking bridge or synchronous backend contract is required; waking a task cannot suspend a C callback. |
| `task::block_on`, synchronous open/close/formatting bridges | Existing Embassy synchronous bridge remains; do not use it to wait for hardware in a new polling hook. Audit executor/task storage independently. |

`core::task::Poll` drives a Rust future. Descriptor `poll/select` reports readable,
writable, error and hangup readiness across descriptors: that public API and its
flags belong to #382 and must reuse device wakeups rather than periodically scan
or embed Rust wakers in the userspace/WASM ABI. General hardware unplug handling,
UART/GPIO/CAN/Wi-Fi/BLE migration and MCU validation remain follow-up work.
