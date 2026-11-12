# Task spawning

`Manager::spawn` keeps its existing signature and accepts non-Send futures.
It selects only registrations for the calling executor. Explicit selection of
another executor returns `InvalidSpawnerIdentifier`; it never transfers an
unconstrained future between executor threads. The shared manager stores
executor identifiers and counters, without storing non-Send spawners. On desktop
and browser targets, registration also remembers a spawner in thread-local
storage for synchronous `block_on` adapters. Bare-metal bootstrap and synchronous
adapters can instead use `spawn_on` with a caller-owned local `Spawner`.
The `run` and `test` macros do this automatically.

Callers needing another executor must arrange for the spawn operation to run on
that executor. In particular, Core's network test initialization currently passes
a newly created thread's identifier to `mount_interface`; that setup needs an
executor-owned bootstrap operation rather than passing its identifier across
threads. The network tests consequently fail closed with this implementation.
No audited Platforms or Applications runtime call explicitly requests a foreign
executor. Bare-metal synchronous HTTP/HTTPS adapters in Platforms' shared driver
must pass a local spawner to `spawn_on` instead of calling `spawn` under `block_on`.

`try_get_current_task_identifier` returns `ThreadNotRegistered` for unknown tasks.
The compatible infallible lookup panics instead of granting root authority.
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
