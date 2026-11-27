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

## Theme adjustments and light rendering

Xila uses the shadcn theme that `lvgl-rust-sys` ships, and changes two things on top of it ([`src/theme/adjustments.rs`](src/theme/adjustments.rs)). LVGL redraws a whole object when a style for one of its states comes or goes, even if nothing looks different, so two of the theme's styles cost far more than they show:

- Its state transition is also added for the pressed state. On an object that has no pressed look, such as a window, a card or a text area, every press and release then redraws the object. Xila removes that copy; the transition for the default state already covers both directions.
- Its style for pressed keys redraws the whole on-screen keyboard twice for each key. Xila removes it and colors the pressed key while it is drawn.

`Manager::set_rendering_mode(RenderingMode::Light)` further reduces what the theme costs on a slow display bus or processor. It is off by default; a platform selects it once, before it creates the interface. Selecting `RenderingMode::Default` again gives every object the theme's look back. In light mode:

- State changes show in one frame instead of a 150 ms transition.
- The scrolls that LVGL animates take one frame: scrolling to a position or an object, settling on a snap point, returning from an overscroll. Scrolling under the finger stays, and its momentum covers the same distance in half the frames on a display that renders slower than LVGL's animation timer runs.
- The end of a scroll does not redraw the scrolled object: the scrollbar keeps one opacity.
- A modal backdrop does not dim what is behind it.
- Lists and tables do not clip their children to their rounded corners, which takes two intermediate ARGB8888 buffers for each redraw.

The theme has no shadows or gradients, and the mode keeps its rounded corners and the half-transparent look of disabled controls, which LVGL draws without an intermediate layer. `src/theme/adjustments/tests.rs` renders fixed pointer scenarios on a 480×320 display and prints what each of them redraws with the theme alone, with the fixes and in light mode: run it with `--nocapture`, in a release build to compare render times.

`invalidation::get_invalidated_class_name` names the class of the object behind an invalidated area, for rendering telemetry.

## Window events

Each window queues the events of its widgets in a fixed ring of `WINDOW_EVENT_QUEUE_CAPACITY` (16) events that lives inside the window object: no allocation, and it never grows. The window handles widget creation and deletion internally and never queues hover, drawing, hit test, invalidation or layout notifications. The rest falls in four tiers, from the least to the most important:

| Tier | Events | Coalesced |
|---|---|---|
| Low | the scroll family (`Scroll`, `ScrollBegin`, `ScrollThrowBegin`, `ScrollEnd`), `Refresh`, `Create` and the filtered notifications | no |
| Transient | `Pressing`, `LongPressedRepeat` | yes |
| State | `ValueChanged`, `SizeChanged`, `StyleChanged` | yes |
| Lifecycle | everything else: `Pressed`, `Released`, `PressLost`, the clicks, `LongPressed`, `Key`, `Focused`, `Defocused`, `CloseRequested`, `Delete`, `Ready`, `Cancel`, `Custom1`, `Custom2`... | no |

Overflow policy, applied when an event is pushed:

1. A transient or state event replaces the queued event of the same kind and target, wherever it is, and moves to the back with the latest data ("latest wins"). It never moves across a lifecycle event of its own target, so the event order of a press is kept: a value change queued before a `Released` or a `Key` of its widget stays, and the newer one is queued after it. The match ignores the input device, so the moves and value changes two devices make on one widget merge into the latest one: consumers read the widget's current state, and events do not carry their device, which would cost a word per slot. A burst therefore never drops the last value, size or style change of a widget: a resize survives any number of scroll events.
2. If the ring is full, the arrival evicts the oldest low event, or else the oldest transient event, or else the oldest state event, but never an event more important than itself. Lifecycle events are never evicted or reordered. An arrival that finds nothing to evict is rejected.
3. Presses are followed per input device: LVGL passes the device with every press event, a device presses one widget at a time, and it may end the press on another widget (a keypad releases on the widget focused at that time). A `Pressed` event is queued only if the ring keeps room for the rest of its press: four slots, enough for `Released`, `ShortClicked`, `SingleClicked` (or `DoubleClicked`, `TripleClicked`) and `Clicked`, for `LongPressed`, `Released` and `Clicked`, or for `PressLost`. Other lifecycle events cannot take that room, so the end of an accepted press always gets in. If the `Pressed` event is rejected, the rest of that press is dropped too: the queue never drops a `Pressed` event while keeping the end of its press. LVGL itself may still deliver an end to a widget that never got `Pressed`: after a drag onto another widget without press lock, or for a keypad released on a newly focused widget.
4. The reservation is given back as the press ends: at `Clicked` or `PressLost`, or else when LVGL never ends the press. Such presses are ordinary: an encoder long press that leaves edit mode, a keypad whose focus moved to another window during the press, a widget disabled while pressed, an input device reset or deleted mid-press. Each would keep four slots, so the window forgets a press as soon as LVGL shows it is over: when its device is released (and not being read), deleted or reset, checked whenever a `Pressed` event arrives and whenever the consumer pops an event; at the next press of the same device; at the next press of any device after a release without a click (a scroll); and when the pressed widget is deleted. Forgetting a rejected press is safe too: a released device has already sent the end that the queue dropped. `Window::discard_events_for_target` removes a widget's events but keeps its press events, except moves and repeats, with a null target, so that the end of a press never arrives without its `Pressed`. Press events sent by code, without an input device, are queued by tier only.
5. Up to four devices are followed at once. A fifth pressing at the same time makes the queue forget the oldest accepted press (only its reservation is lost), or else the oldest rejected press, whose end could then get through.

`OwnedWindow::dropped_event_count` counts every evicted or rejected event. Losses are logged at most once per second (`LOSS_REPORT_INTERVAL`, in LVGL ticks) with the number lost since the previous report, both when events are pushed and when the consumer pops them, so a consumer that never empties the queue keeps getting periodic summaries.

RAM budget (excluding allocator bookkeeping): on x86_64, `Event` is 32 bytes,
so the old queue started at 32 bytes inline plus a 320-byte allocation, and
could grow indefinitely. The new queue is 632 bytes inline (16 slots, indices,
four 24-byte press records and the loss counters), with no separate
allocation or growth. The whole host `Window` is 744 bytes. On 32-bit targets
the corresponding layout is expected to be 16 + 280 bytes before and about
516 bytes after; verify target layouts when budgeting board RAM. This trades a
slightly larger initial reservation for a fixed upper bound and fewer heap
blocks.

## Window ownership

A window has one owner, its `OwnedWindow`. Drop takes the graphics lock if it is free. If the lock is busy, drop queues the window for deletion when a session next drains deferred work. `OwnedWindow::close().await` waits for the lock and deletes the window before returning. `OwnedWindow::delete(lv)` deletes it during a session.

- The window class is `ChildrenOnly` in lvgl-rust's external class policy: safe code can build and delete a window's content, but the safe layer refuses to delete, clean away, own, reparent or reorder a window or anything that contains one. Core's other classes (the file picker) are fully protected.
- The owner's event and icon methods require the session token. The owner does not expose references to the window or its LVGL object. `register_waker` and `wake_up` access only the atomic waker and can run without a session.
- Taking ownership of a raw window (`OwnedWindow::from_raw`) and changing the window parent (`Manager::set_window_parent`) require `unsafe`.

Code that takes a window out of its owner (`OwnedWindow::into_raw`, as the Wasm host does) becomes its owner and deletes it once, under the graphics lock.

An event's target may have been deleted, by safe code too, after the window queued the event. `Event::is_target` compares addresses. To use the target, `Event::find_target_in(root)` returns it only once found in `root`'s live tree; never keep a target otherwise.

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
Code that already holds a `GraphicsGuard` can use `guard.with_lvgl(|lv| ...)`.
Create windows and look up, move or close them inside the session. Use the
same session for a lookup and any raw operation on the returned object.
Raw calls that delete or move objects must never overlap safe views.
Do not call raw `lv_init`, `lv_deinit`, or change object IDs.

### Call executor

`graphics::call` is the one service behind every way a program reaches the UI. A caller names a call by its catalog ID (`call::Function`; IDs come from `bindings_generator`'s `CallCatalog`, extensions from `0xf000`) and passes up to seven words. `call::execute(&mut State, &mut dyn Transport, &Call)` checks the call, takes the graphics lock for that one call, runs it on LVGL and writes the result back. A caller on a cooperative executor waits for the lock itself, by suspending, and runs the call with `call::execute_locked(&mut guard, ...)`. `State::release_locked` takes the same guard and deletes the process's objects.

| Piece | Owner |
| --- | --- |
| Graphics lock | the executor: one call holds it, nothing is held between calls |
| Object handles | `call::State`, one per process |
| Event queue | the window's queue; `window_pop_event` and `window_pop_pointer_event` translate to the Xila event contract (`call::EventCode`) |
| Extension calls | the executor (IDs from `0xf000`) |
| Argument memory | the caller's `call::Transport` |

**Transport.** Only pointer, string and buffer words differ between callers. A `Transport` borrows bytes at an address, mutably or not, and a NUL-terminated string. Wasm guests pass offsets into their linear memory: `LinearMemory` translates and bounds-checks them, with a 32-bit or 64-bit word as the guest has. Native code passes host pointers, since native code is trusted. Values behind an address are little-endian. A call word is a `usize`, so a call costs no double-word arithmetic on a 32-bit target (a 64-bit guest only runs on a 64-bit host).

**State.** Each process owns a `State`: the sorted array of its live handles (the ID LVGL keeps in each object's own `id` field) and a sorted side table for what LVGL borrows from the host (button maps, grid descriptors, canvas buffers). A handle from another process, or of a deleted object, fails as `InvalidHandle`. Dropping the `State` deletes everything the process still owns, under the lock; the free observer releases side data when LVGL deletes an object by itself, such as the children of a deleted parent.

**The call table is generated.** `build.rs` runs `graphics_bindgen::executor` over the catalog and the ABI declaration in `graphics_bindgen::abi`, the same declaration that generates the guest bindings. The catalog is parsed from the bindings `lvgl_rust_sys` already generated for the build target (its `BINDINGS` build metadata), so building graphics runs no second LVGL bindgen pass and builds no host copy of LVGL. The output is the `Function` enum, a packed table of IDs, argument counts and result kinds, and one `match` on the call. Constructors and scalar setters and getters expand into calls of a few shared helpers; custom adapters call a named handler; everything else falls through to the handwritten match in `call/handlers.rs`. To add a call, declare it in `abi.rs` and write a handler only if no generated shape fits.

### Wasm host integration

Applications' Wasm host is a thin transport over the executor: it hands the guest's linear memory over as a `LinearMemory` and keeps one `call::State` per VM. Native executables reach the same executor through `xila_graphics_call`, the one graphics slot of the native ABI (`libraries/abi/application`), whose transport reads host pointers in place and scans strings with a bound; its `State` lives in the process's host-owned state and is allocated by the first call. The SDK's `xila/graphics.h` and `xila_sys::graphics` bindings are generated by `graphics_bindgen::guest` from the same declaration, with the names and types of the Wasm guest API. What follows is what the executor relies on from `lvgl-rust`.

Each process keeps its own table of handles and gives the caller the IDs, never raw pointers, like file descriptors (see lvgl-rust's
`host-several-clients` guide). Resolve every ID through the table of the process that sent it; an ID from another process, or of a deleted object, fails
as `Dead`. Insert only objects the process created beneath its own private root;
shared windows and display layers stay host-owned and out of the table.

Install the single `Lvgl::set_free_observer(Some(callback))` (`free-observer`
feature) to learn about objects LVGL deletes on its own, such as children
deleted with their parent. It runs at the end of LVGL destruction, including
cascading raw deletions, and must not call LVGL or re-enter graphics: record
`Freed::handle().into_raw()` and release retained side-table data after the
LVGL work. `WidgetExt::find_handle` maps a live object back to its handle via
its object ID in O(1), without a per-object callback. At teardown, delete the
VM's root under the graphics lock and drop its table.

The registry starts with 32 static slots and grows in non-moving doubling
chunks from the global allocator; published chunks remain allocated for the
process lifetime so deferred drops can address them safely. A handle is 4
bytes, a slot 8 bytes on 32-bit targets (16 on 64-bit), plus LVGL's object-ID
pointer (4/8 bytes). Its 16-bit generation detects a stale handle until 65,536
reuses of the same slot; handles are not cryptographic capabilities. A VM's
table adds about 4 bytes per object in its own memory. The safe crate's
default remains allocation-free; Core explicitly enables `growable-registry`
and `free-observer`, without `heap`.

### Updating consumers

Consumers with a direct sys dependency must take it from crates.io, as Core does:
`lvgl_rust_sys = { package = "lvgl-rust-sys", version = "0.3.1" }` from
crates.io. Cargo allows only one package with `links = "lvgl_rust_sys"` in a
graph, so a consumer cannot keep a Git copy of the bindings once it uses a Core
that takes them from crates.io. Switch the direct dependency in the same change
that relocks Core. Keep the dependency key to preserve `use lvgl_rust_sys`
statements.
Their own `lv_conf.h` must enable `LV_USE_OBJ_ID=1`, `LV_OBJ_ID_AUTO_ASSIGN=1`, and
`LV_USE_OBJ_ID_BUILTIN=0`,
or builds must set `DEP_LV_CONFIG_PATH` to Core's `include` directory. There
must be exactly one matching config for raw bindings, LVGL, safe wrappers,
fonts, and the guest/host generator.

The guest catalog excludes the seven object-ID functions newly enabled by
this migration. Its fixture records every legacy ID from the prior sys
revision with Core's configuration; Xila helpers retain their `0xf000+` IDs.
