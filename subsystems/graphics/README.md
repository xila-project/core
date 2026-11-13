# Graphics

Xila's graphics subsystem: it owns LVGL, the displays and input devices, the window manager and the theme.

## LVGL's memory

LVGL allocates a lot of small, long-lived blocks (objects, styles, event descriptors, text). Where they come from is selected by `XILA_LVGL_MEM_POOL` in [`include/lv_conf.h`](include/lv_conf.h):

| Setting | LVGL allocates from | `lv_mem_monitor()` reports |
| --- | --- | --- |
| `XILA_LVGL_MEM_POOL=0` (default, every target) | Xila's system allocator (`LV_STDLIB_CUSTOM`, hooks in [`src/stubs.rs`](src/stubs.rs)) | the whole system heap: total, free and used percentage only |
| `XILA_LVGL_MEM_POOL=1` (opt-in) | LVGL's own TLSF allocator over one fixed pool (`LV_STDLIB_BUILTIN`) | the pool: totals, block counts, biggest free block, fragmentation and `max_used` |

The system allocator is the default: LVGL takes only what it uses, nothing is reserved on boards without a UI, and the UI can grow at peaks. The pool is for boards that can dedicate a memory region to the UI, for example PSRAM or a separate SRAM bank (place it with `LV_MEM_POOL_INCLUDE`/`LV_MEM_POOL_ALLOC` or `LV_MEM_ADR`): UI churn then neither fragments nor starves the system heap, and a UI that outgrows its pool fails in LVGL (`lv_malloc()` returns null) instead of taking memory from the rest of the system. Window event queues (see below) live inside LVGL objects, so they are part of the pool too.

Both settings are passed to the LVGL build through `LVGL_CFLAGS` (comma-separated `NAME=VALUE`), for example in the `[env]` table of a board's `.cargo/config.toml`:

```toml
[env]
LVGL_CFLAGS = "XILA_LVGL_MEM_POOL=1,XILA_LVGL_MEM_SIZE=196608"
```

| Setting | Default | Meaning |
| --- | --- | --- |
| `XILA_LVGL_MEM_POOL` | `0` on hosts, `1` elsewhere | `0` selects the system allocator, `1` the fixed pool. Force `1` on a host to measure LVGL's own usage or to run the tests against the pool. |
| `XILA_LVGL_MEM_SIZE` | `65536` (64 KiB) | Pool size in bytes, at least 2048 and a multiple of 8. By default the pool is a static array in `.bss`. |

`build.rs` preprocesses the selected C configuration with LVGL's exported compiler settings and enables `lvgl_system_heap` only for `LV_STDLIB_CUSTOM`. A compile-time assertion in `stubs.rs` checks this against the generated bindings. Overrides in a consumer header are respected too.

Cargo reads `DEP_LV_CONFIG_PATH` from the invoking workspace's `.cargo/config.toml`: Core selects `subsystems/graphics/include`, while Platforms and Applications select their respective `config/lvgl` directories. The latter headers currently select `LV_STDLIB_CUSTOM` unconditionally; to offer the optional pool, they must adopt this memory section in their own repositories. Core cannot override those consumer headers. `LVGL_CFLAGS` reaches both C compilation and bindgen through `lvgl_rust_sys`; the same exported settings compile Core's fonts.

### Placing the pool

A pool in `.bss` lives in internal RAM. To put it elsewhere (for example in PSRAM), let the platform allocate it once, when LVGL initializes, by defining in the board's `lv_conf.h` (or through `LVGL_CFLAGS`):

```c
#define LV_MEM_POOL_INCLUDE "board_memory.h"
#define LV_MEM_POOL_ALLOC(size) board_lvgl_pool_allocate(size)
```

`LV_MEM_POOL_ALLOC` must return `XILA_LVGL_MEM_SIZE` bytes, aligned to 8, that live for the whole run. Alternatively set `LV_MEM_ADR` to the pool's fixed address.

### Sizing the pool

Start from the default and measure; the right size depends on the display, the color depth and the UI, so there is no universal value. What counts against the pool:

- the objects, styles, event descriptors and text of every live window, plus LVGL's own state;
- LVGL's temporary buffers: layers for opacity, shadows and transforms, and snapshots are sized by what they draw, so the heaviest screen decides;
- the transient peaks while a screen is built or torn down, which are higher than the steady state.

The display's draw buffers are Rust allocations made when the display is created, so they are not part of the pool. System heap statistics (including the Activity app) continue to describe the system allocator. A static LVGL pool reserves RAM outside that heap; use `lv_mem_monitor` to observe its occupancy separately. A platform-allocated pool appears as one long-lived reservation in the platform allocator.

Measure the peak with `lv_mem_monitor` (available with the pool selected):

```rust
let mut monitor = graphics::lvgl::lv_mem_monitor_t::default();
unsafe { graphics::lvgl::lv_mem_monitor(&mut monitor) };
// monitor.max_used: high-water mark since lv_init(), in bytes
// monitor.total_size - monitor.free_size: in use now
// monitor.free_biggest_size / monitor.frag_pct: fragmentation of what is left
```

Open every screen and application that matters, including the largest ones, then read `max_used` and add about 25 % headroom. `max_used` approaching `total_size`, or `free_biggest_size` much smaller than `free_size`, means the pool is too small or too fragmented.

A full-screen ARGB8888 capture at 320×240 alone needs about 300 KiB of pixel storage, so the default pool is intentionally insufficient for that operation. RGB565 needs half the pixel storage.

Testing the pool on a host: `mise run test:graphics-pool` runs this crate's tests with the pool forced on.

### Strings

LVGL releases the strings it duplicates (`lv_strdup`, `lv_strndup`) with `lv_free()`, so with the pool they are copied into the pool, not into the system heap. Memory handed to LVGL by the system allocator must never be released with `lv_free()`, and the reverse.

## Window events

Each window queues the events of its widgets in a fixed ring of `WINDOW_EVENT_QUEUE_CAPACITY` (16) events that lives inside the window object: no allocation, and it never grows. Overflow policy, applied when an event is pushed:

1. A pointer move (`Pressing`) that directly follows a pointer move for the same target replaces it, so only the latest position is kept.
2. If the ring is full, the oldest queued pointer move is dropped to make room: key, click, press and release events are never lost while a move can be sacrificed.
3. If it is still full, the new event is dropped, which keeps the order of the queue and its front stable.

Every dropped event is counted; read it with `Window::dropped_event_count()`. A consumer that falls behind by more than 16 events loses events, so drain the queue every time the window wakes it.

RAM budget (excluding allocator bookkeeping): on x86_64, `Event` is 32 bytes,
so the old queue started at 32 bytes inline plus a 320-byte allocation, and
could grow indefinitely. The new queue is 520 bytes inline (16 slots plus
indices and a saturating loss counter): 168 more bytes than the old initial
capacity, with no separate allocation or growth. The whole host `Window` is
624 bytes. On 32-bit targets the corresponding layout is expected to be
16 + 280 bytes before, 456 bytes after; verify target layouts when budgeting
board RAM. This trades a slightly larger initial reservation for a fixed
upper bound and fewer heap blocks.

## LVGL bindings (lvgl-rust)

Graphics builds LVGL 9.6 through `lvgl-rust-sys`, using `include/lv_conf.h`.
The existing `graphics::lvgl` module keeps the raw API and Xila helpers.
Allocator hooks, locale fonts and the shadcn theme remain owned by Core.
The safe API is re-exported as `graphics::lvgl_rust`.

### Safe sessions

Use `manager.with_lvgl(|lv| { /* safe LVGL calls */ Ok(()) }).await?`,
or `with_lvgl_sync` in synchronous code. The private guard implements
`LvglAccess` using the same mutex as `Manager::lock`; views cannot escape
this closure. Store a `Handle<W>` across calls and resolve it in each session.
Code already holding the graphics lock can use unsafe `with_lvgl_locked`:
it must hold that lock throughout and must not overlap another session.
Raw calls that delete or move objects must never overlap safe views.
Do not call raw `lv_init`, `lv_deinit`, or change object IDs.

### Future Wasm host integration

Assign each live VM a distinct `Owner::new(id)` (default: 1..=15).
Mint `WidgetExt::handle_for(owner)` only for objects created by that VM;
use `Lvgl::resolve_for::<Obj>(raw, owner)` on every guest-provided handle.
A raw handle alone is not an authority: the dispatcher must supply the VM's
trusted owner. Shared windows and display layers must stay host-owned.

Install the single `Lvgl::set_free_observer(Some(callback))` to release
retained side-table data by `Freed::handle().into_raw()`. It runs at the end
of LVGL destruction, including cascading raw deletions. It must not call
LVGL or re-enter graphics. Pointer-to-handle lookup through a live object's
`handle_for` uses its object ID in O(1), without a per-object callback.
At teardown call `Lvgl::delete_owner(owner)` under the graphics lock.
This deletes tagged roots and their descendants; structurally protected
objects are untracked rather than deleted. Keep VM objects beneath a private,
deletable root and delete it before reusing the owner ID. Retained resources
for refused deletions need explicit host cleanup.

The registry starts with 32 static slots and grows in non-moving doubling
chunks from the global allocator; published chunks remain allocated for the
process lifetime so deferred drops can address them safely. A handle is 4
bytes, a slot 8 bytes on 32-bit targets (16 on 64-bit), plus LVGL's object-ID
pointer (4/8 bytes). Owner tags use four generation bits at no extra slot
cost: 12 bits remain for reuse detection. Handle values can repeat after
4096 reuses of the same slot; do not treat them as cryptographic capabilities.
The safe crate's default remains allocation-free; Core explicitly enables
`growable-registry` and `owners`, without `heap`.

### Updating consumers

Consumers with a direct sys dependency must use `package = "lvgl-rust-sys"`
with the same git source and branch as Core's `lvgl_rust_sys` dependency.
The old package `lvgl_rust_sys` cannot coexist with the new one: both link
LVGL. Keep the dependency key to preserve `use lvgl_rust_sys` statements.
Their own `lv_conf.h` must enable `LV_USE_OBJ_ID=1`, `LV_OBJ_ID_AUTO_ASSIGN=1`, and
`LV_USE_OBJ_ID_BUILTIN=0`,
or builds must set `DEP_LV_CONFIG_PATH` to Core's `include` directory. There
must be exactly one matching config for raw bindings, LVGL, safe wrappers,
fonts, and the guest/host generator.

The guest catalog excludes the seven object-ID functions newly enabled by
this migration. Its fixture records every legacy ID from the prior sys
revision with Core's configuration; Xila helpers retain their `0xf000+` IDs.
