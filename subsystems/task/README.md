# Task spawning

`Manager::spawn` keeps its existing signature and accepts non-Send futures.
It selects only registrations for the calling executor. Explicit selection of
another executor returns `InvalidSpawnerIdentifier`; it never transfers an
unconstrained future between executor threads. On desktop and browser targets,
registration also remembers a spawner in thread-local storage for synchronous
`block_on` adapters. Bare-metal bootstrap and synchronous adapters can instead
use `spawn_on` with a caller-owned local `Spawner`. The `run` and `test` macros
do this automatically.

`spawn_on_executor` takes the registration identifier of a specific executor.
The closure, future and result must implement `Send`. The task stays on that
executor until it exits.

`spawn_send` targets any registered executor: an explicit identifier, or the
registration with the fewest live tasks (lowest identifier on ties). It needs
neither an Embassy waker nor a local spawner, so it also works under `block_on`.
Its closure and result must be `Send`; the closure runs on the target executor,
so the future it returns may be `!Send`, as with Embassy's `#[task]` functions
and tokio-util's `spawn_pinned`. The shared manager stores, per registration,
the executor identifier, a live-task counter and Embassy's `SendSpawner`; it
never stores a non-Send `Spawner`.

Both paths share one task storage pool. Embassy allows a finished
`TaskStorage` to be spawned again on a different executor. Claiming atomically
requires a slot that is neither spawned nor run-queued: the executor that ran
the previous future has dropped it and dequeued any pending wake. Later stale
wakers and timer entries cause at most a spurious poll on the slot's current
executor; slots are never freed, so the headers they touch stay valid. A separate `Send` pool would duplicate the poll code and retain
peaks of both kinds of task separately. Embassy requires `Send` on the stored
type only to enqueue through `SendSpawner`, so the private storage future
asserts it: local futures never leave their executor thread, and `spawn_send`
futures cross threads once, before their first poll, holding only the checked
`Send` closure, result signal, manager reference, identifiers and name.

Core's network tests use this: their root and loopback runner live on a
dedicated executor thread, and test-harness tasks start socket tasks there with
`spawn_send`. `mount_interface` still spawns its runner on the calling
executor, so its bootstrap runs on the target executor through `spawn_send`.

`try_get_current_task_identifier` returns `ThreadNotRegistered` for unknown tasks.
The compatible infallible lookup panics instead of granting root authority;
Core's production callers use the fallible lookup. Tasks spawned directly with
an Embassy spawner, `#[embassy_executor::task]` functions and interrupt
executors are unknown, except as a fresh manager's first lookup once any
managed task has been polled in the process (which records Embassy's waker vtable).
An independent manager can adopt its first managed Embassy task as root; root
cannot be recreated after exit. `get_current_internal_identifier` now returns
`Result<usize>` and rejects foreign wakers, including `block_on`'s no-op waker.
Embassy 0.10's fallible waker helper is private. A wrapper polled exclusively by
Embassy records the canonical waker vtable; lookups compare it before reading the
public opaque waker data pointer. This also preserves full-width identities:
Embassy's `TaskRef::id` and `SpawnToken::id` truncate pointers to 32 bits on hosts.

All task futures share one boxed, type-erased storage type. Slots grow in chunks
of two, only after existing slots return `Busy`, up to `with_maximum_tasks`'s
limit (default 256, including root). Slots remain allocated because Embassy
requires their headers to outlive stale wakers, timer queue entries and queued
exit polls. Embassy drops each boxed future on completion, then releases its
slot. The live-task mapping is installed before enqueueing, and metadata cleanup
precedes join completion. Active spawner registrations cannot be removed:
`unregister_spawner` returns `AlreadySet` until their task counter reaches zero.

The allocation regression warms the maps and storage, then runs 1,000 sequential
tasks. On x86_64, an equivalent main-branch workload retained 168 bytes per spawn
with three allocations per spawn. Reusable storage uses 96-byte slots and two
allocations per spawn (result signal and boxed future), retaining zero additional
bytes after warmup. Chunk and slot-index allocations occur only as peak occupancy
grows; metadata nodes and inherited environment variables can add allocations.
Environment inheritance still clones the parent's vector to preserve its API.

The browser yield delay remains 10 ms. Embassy 0.10's wasm pender uses
`Promise.then`, so a continuously self-waking yield can starve rendering, input
and timer macrotasks. Replacing the delay requires a platform macrotask primitive
and browser fairness/latency validation.

# Activity metrics

The opt-in `metrics` feature (`xila/task_metrics`) times every poll of a
managed task with a monotonic clock: the operating system's on Linux, macOS
and Windows (lock-free, nanosecond resolution), and Embassy's elsewhere, the
one each platform already provides for `sleep`, so it works under `no_std` on
every target. Each task records its poll count, busy time and longest poll;
each executor (a core or an operating system thread) records its busy time,
plus its idle time when the platform reports it. `Manager::get_metrics`
returns a `MetricsSnapshot` of these monotonically increasing counters with
timestamps on the same clock; applications compute CPU shares from the
differences between two snapshots. Without the feature it returns `None`, and
the platform hook only runs its closure.

Busy time is wall-clock time inside polls, not processor time: on hosts, time
a thread is descheduled or blocked during a poll counts as busy. On hosts the
metrics clock counts from its own first read, not from `embassy_time::Instant`'s
origin, so compare metrics timestamps only with each other.

Task counters live in a parallel array beside the retained storage slots and
are reset when a slot is claimed, so spawning and polling allocate nothing.
Executor counters are allocated once per executor and kept. Writers are
always the executor that owns the counters; readers use a sequence lock built
from 32-bit atomic loads and stores only, so it works without compare-and-swap
or 64-bit atomics (Cortex-M, ESP32 and ESP32-C3). Readers spin while a write
is in progress, outside any critical section: never read the metrics from an
interrupt handler or from an interrupt executor that preempts an instrumented
executor on the same core.

Writers read the clock inside the write section, which keeps readers from
extrapolating a finished period on one core and on x86. On weakly ordered
multi-core targets (AArch64, RISC-V), fences do not order a system-register
clock read after a store, so a reader on another core can still extrapolate a
period in progress by up to the writer's store latency. Executor busy time,
idle time and timestamps never decrease because readers clamp each sample to
a per-executor high-water mark, updated with plain loads and stores in a short
critical section.

Executor busy time is the sum of its task polls and needs no platform code.
To measure idle time, executor loops create the hook on the executor's thread
before the loop and wrap their wait for work. Once the hook exists the
executor reports idle time, starting at 0; without it, idle time is `None`.
The hook is created from the executor's `Spawner` and is `!Send`, so it stays
on the executor's thread, the counters' only writer:

```rust,ignore
let activity = task::metrics::ExecutorActivity::new(executor.spawner());
loop {
    unsafe { executor.poll() };
    activity.measure_idle(|| cortex_m::asm::wfe());
}
```

For a monitoring application such as Activity, compute every difference with
`saturating_sub`:

- Match executors between snapshots by `executor`, not by `spawner`.
- Core load is `1 − Δidle / Δtimestamp` when idle time is reported, and
  `Δbusy / Δtimestamp` otherwise, using the executor's own `timestamp_nanoseconds`.
  Clamp it to `[0, 1]`: each counter is converted to nanoseconds separately,
  rounding down, so with a clock rate that does not divide one billion, such as
  32,768 Hz, `Δidle` can exceed `Δtimestamp` by 1 ns over the same ticks, and
  cross-core extrapolation can add a little more.
- A task's share is `Δbusy / Δtimestamp` with the snapshot timestamp. Busy time
  is added when a poll ends, so a long poll counts entirely in the window where
  it ends. Group tasks by `executor` and, when one executor's shares sum above
  1, divide them by that sum instead of clamping each task.
- Adopted tasks have no counters, report `spawner == usize::MAX` and no
  `executor`.

The remainder, elapsed time minus busy and idle time, is executor bookkeeping
and, on hosts, time the thread was descheduled outside polls. Core's test
executor uses the hook. The Linux-only `profiling` feature is unchanged; it
adds per-thread CPU clocks, allocation attribution and finished-task history
for recordings, and both features can be enabled together.
