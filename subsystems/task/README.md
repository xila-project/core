# Task spawning

`Manager::spawn` keeps its existing signature and accepts non-Send futures.
It selects only registrations for the calling executor. Explicit selection of
another executor returns `InvalidSpawnerIdentifier`; it never transfers an
unconstrained future between executor threads. On desktop and browser targets,
registration also remembers a spawner in thread-local storage for synchronous
`block_on` adapters. Bare-metal bootstrap and synchronous adapters can instead
use `spawn_on` with a caller-owned local `Spawner`. The `run` and `test` macros
do this automatically.

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
