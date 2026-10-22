# Wasmi runtime

`wasm` is the Wasmi-based runtime and includes the guest graphics wrappers.
It replaces the original WAMR implementation. Xila's shared `EventCode` enum
defines guest event values independently of LVGL. The host translates native
event variants into that contract. Graphics call IDs and state/style definitions
still follow the bundled LVGL API, so changes to those interfaces may require
rebuilding the host and guest applications together.

Wasmi 2 runs with default features disabled for `no_std` hosts. Both the runtime
and translation test harness explicitly enable validation, memory64 support,
and automatic dispatch, which falls back to portable dispatch when required by
the target or optimization level.

`--instruction-limit` supplies the fuel budget for each cooperative execution
slice. Wasmi 2 meters input Wasm operators instead of its internal bytecode, so
the same budget may yield at different intervals than with Wasmi 1. A bulk
operation can require more fuel than one normal slice; the runtime replenishes
at least the amount required to resume it.
Functions are translated eagerly before execution to avoid non-resumable fuel
exhaustion during lazy callee translation in Wasmi 2.0.0 (tracked by
[Wasmi PR #2032](https://github.com/wasmi-labs/wasmi/pull/2032)). This also
translates unused functions, which can increase startup work and compiled-code
memory.

## Test Calculator

From the repository root on Linux:

```sh
rustup target add --toolchain stable wasm32-wasip1

# Headless acceptance: builds the real guest, enters 1 + 2 = through LVGL
# button events, checks the displayed result, requests close, checks cleanup.
cargo +stable test -p wasm --features graphics \
  --target x86_64-unknown-linux-gnu --test calculator

# Interactive native display. Click calculator buttons; press Enter in the
# launching terminal to request close and let the test finish successfully.
RUSTUP_TOOLCHAIN=stable cargo test -p calculator \
  --target x86_64-unknown-linux-gnu --test calculator_test \
  -- --ignored --nocapture
```

The interactive test requires a working native graphical session. Its terminal
close path sends Xila's window-close event, rather than abruptly killing the VM.
The guest handles that event as a numeric code, not an invalid Rust enum variant.

Other focused tests:

```sh
cargo +stable test -p graphics_bindgen --target x86_64-unknown-linux-gnu
cargo +stable test -p wasm --features graphics --target x86_64-unknown-linux-gnu
cargo +stable test -p wasm --no-default-features \
  --features host,graphics,memory_64 --target x86_64-unknown-linux-gnu
```

Calculator is a wasm32 guest, so its acceptance test is not run in the memory64
configuration. Core/adapter tests cover both guest-word widths. Portable memory
tests, 100% line/function coverage commands, and Miri commands are documented in
[`tests/translation/README.md`](tests/translation/README.md).

## Graphics architecture

Pixel Painter uses the reusable indexed-canvas helpers. `indexed_canvas_create`
allocates a host-owned image; `indexed_canvas_set_pixels` validates and copies
palette indexes and RGB888 palette entries from guest memory. The per-VM image
budget is 65,536 pixels. Buffers remain alive through native object deletion and
are reclaimed afterward or at VM teardown.

`window_pop_pointer_event` extends event polling with event-time x/y coordinates
relative to the target content area and its width/height. For events without
pointer data it writes `i32::MIN` to x/y and zero to width/height. The original
`window_pop_event` contract is unchanged. Textarea copying, keyboard attachment,
padding, and flag removal also have checked adapters. New Xila helpers have
append-only IDs starting at `0xf000`, preserving existing call numbering.

Run the headless Wasm app acceptance and ABI boundary tests with:

```sh
cargo test -p wasm --features graphics,xila/graphics_rendering_xrgb8888 \
  --test graphics_canvas --test pixel_painter
```

- `subsystems/graphics/bindings_generator` discovers and orders one shared LVGL ABI
  catalog; unsupported calls keep their IDs. Wasmi's supported-call table checks
  full C signatures, not just argument counts. The catalog is compatible with
  guest bindings built from the same LVGL revision/configuration, not a stable
  cross-version ABI.
- `src/host/graphics/{dispatch,objects,marshal}.rs` use explicit graphics state
  and checked borrowed guest memory. They do not use Wasmi types or global
  graphics-manager lookups. The thin `graphics/wasmi.rs` adapter alone handles
  imports, exports, memory/store borrowing, and context capability checks.
- Object handles are local to the VM. Deleted handles are never reused during
  execution. Native deletion notifications invalidate handles and remove stale
  queued event targets. Teardown deletes owned windows and their children using
  the same context-provided manager and lock, including on traps.
  Registration clears matching pre-existing queued targets to null before
  issuing a new handle, so native address reuse cannot grant an old unknown
  event a new capability. Event codes are preserved.
- LVGL copies label strings synchronously. Button-map strings and the native
  pointer table are copied into stable VM-owned storage because LVGL retains
  them. Guest memory growth/mutation does not invalidate the map. A replacement
  switches LVGL to the new map before releasing the old map.
  Deleted widgets' maps are reclaimed after native deletion returns (or on the
  next dispatch for external deletions); only small handle tombstones persist.

Enable the optional `graphics` feature for host bindings. Select the Xila
rendering feature appropriate to the platform (e.g.
`xila/graphics_rendering_xrgb8888` for a standalone host library check). Normal
non-graphical runtime use does not enable graphics.

Guest-only consumers must disable defaults to avoid enabling the host runtime:

```toml
wasm = { workspace = true, default-features = false, features = ["guest", "graphics", "memory_32"] }
```

`default_guest` additionally enables C wrappers and emits `xila_graphics.h` in
the build output directory. No build step rewrites the guest source files.
Weather's guest and host-test wiring compile against this crate, but graphical
execution still requires APIs beyond the Calculator subset listed below.

### Supported calls and limits

This first milestone supports windows (creation, icon, event pop), object
creation/deletion, position/size and scalar geometry queries, alignment,
layout update, built-in default/flex/grid layout selection, flex flow/alignment/
grow, width/height, flags, validated Grid tracks/placement/alignment, local
background/text/border/radius/opacity/text-alignment/min-max styling, semantic
theme surfaces and host font roles, labels (creation/text), button matrices
(creation/map/selection), range/value/mode/orientation for Arc/Bar/Slider,
Switch orientation/state, Spinner animation parameters/duration/sweep, Dropdown
and Roller with bounded copied options, selection and selected-text queries, and
percentage conversion. Calendar supports validated today/shown dates, bounded
copied highlights, and shown-date output. Spinbox supports bounded range/value/
step/digit-format operations, rollover, increment, and decrement. Most other
widget-specific query/configuration APIs and advanced style properties remain
unsupported. Chart supports bounded series slots and copied values; Line accepts
bounded points copied into host-owned memory because LVGL retains the point
array pointer. List supports copied text sections/buttons with safe child
handles, and Tileview supports bounded tile creation and active-tile navigation.
List text is UTF-8 validated and copied before native calls; the guest never
passes icon, tile, or series pointers to LVGL.
The shadcn-themed slice adds Badge and Card helpers, Button variants, and Input
error states by calling the shared `lvgl_theme_shadcn` theme APIs.

- 256 issued object handles per execution, including subsequently deleted ones.
- 64 map entries including the sentinel; 1024 bytes per NUL-terminated string.
- 8192 retained map-string bytes per widget, including the native sentinel.
- 1024 option-string bytes/64 entries per Dropdown or Roller, with an 8192-byte
  per-VM retained high-water budget shared with Calendar highlights, Line points,
  and List text. Calendar accepts at most 32 dates; Roller infinite mode has a
  512-byte input cap to bound LVGL's repeated internal option storage.
- Spinbox values are limited to ±1,000,000, step to 1..=1,000,000, and digit
  count to 1..=10. Calendar dates use Gregorian validation.
- Chart allows up to 8 series and 128 values per series; Line allows up to 128
  points. List allows 64 child items and Tileview 32 live tiles per view; tile
  coordinates must be unique within a view.
- Theme variants are 0..=4, surfaces are 0..=3, and error flags are boolean;
  palette updates remain owned by the existing display theme integration.
- Null and empty-string guest map sentinels both become LVGL's empty-string
  terminator. Row separators are preserved.

Return status zero means success. Errors are nonzero: invalid count 1, invalid
memory 2, unavailable capability 3, unsupported call 4, invalid handle 5,
resource limit 6, invalid value 7, native failure 8.

Calls requiring arbitrary native structure layouts, guest callbacks, retained
guest pointers, raw font/image/chart handles, or unsupported LVGL APIs return unsupported.
No typed host reference is formed from guest memory.

## Scheduling and remaining limitations

WASI `poll_oneoff` supports up to 64 realtime/startup-clock subscriptions,
relative and absolute deadlines. It validates explicit little-endian records
and outputs, suspends the VM, asynchronously waits, then reacquires memory and
resumes with ready events. FD polling and unsupported clocks return errors.
The startup-clock implementation follows the existing Xila time manager; it is
not an independent hardware monotonic-clock source.

Fuel exhaustion and `sched_yield` yield to the executor. Clock polling uses
resumable Wasmi host calls; suspension is VM-local. WebAssembly start-section
suspension is not supported; WASI command guests should use exported `_start`.
`--install` selects the guest's `__install: () -> ()` export instead of `_start`.
A missing or incorrectly typed install export fails without launching main.
The command-line stack-size option remains unimplemented.

The current graphics ABI audit and staged delivery roadmap are tracked in
[`GRAPHICS_ABI.md`](GRAPHICS_ABI.md). In particular,
the generator currently emits more guest wrappers than the host implements;
unsupported calls return status 4 and should not be treated as available APIs.

Static audit of the current LVGL configuration: 28 surveyed widget families
are enabled, while the guest generator emits 1,074 LVGL/Xila wrappers and the
host supports 92 calls (including Xila helpers). Seven enabled widget families
have creation adapters, but several remain partial. This is not a runtime or
performance benchmark. The next ABI tranches are a shared host/guest support
manifest and inventory, semantic style/font APIs, then the BTreeMap handle
registry and typed bounded events; see issue #483 for scope and acceptance.

The existing stdin fixture passes with a line of input. Running it with closed
stdin (`/dev/null`) exposes an unrelated `drivers_std::StandardInDevice` bug:
it discards the actual read count and reports the buffer length even at EOF.

Host tests and Miri do not establish embedded hardware correctness or firmware
RAM/flash costs. Native LVGL integration tests are separate from the portable
translation coverage gate; no full LVGL/WASI compatibility is claimed.
