# WASM graphics ABI audit and roadmap

This document tracks the guest-facing LVGL ABI, not the browser display backend. It records a source audit of the current checkout; counts are configuration-specific and are not runtime benchmarks.

## Current audit

| Surface | Current state |
| --- | --- |
| Enabled LVGL widget families | 28 surveyed in `subsystems/graphics/include/lv_conf.h` |
| Guest Rust wrappers emitted | Filtered to the 129 currently host-supported calls |
| Host-dispatched calls | 129 after Grid/Flex, local style/font, range, selection, LED, Calendar, Spinbox, Chart, Line, List, and Tileview additions; unsupported IDs return `GraphicsError::Unsupported` |
| Widget families with usable constructors | Button, Buttonmatrix, Checkbox, Label, Tabview, Textarea, Keyboard, Calendar, Spinbox, Chart, Line; Xila Window/Object/IndexedCanvas are extra abstractions |
| ABI IDs | Legacy calls are alphabetically numbered from discovered signatures; Xila extensions use append-only IDs starting at `0xf000` (Grid `grid_fr` is `0xf006`) |
| Object registry | Per-VM `Vec<Entry>`, 256 lifetime-issued IDs, tombstones; full scans occur during reaping, reverse lookup and canvas accounting |
| Per-object adapter allocation | One `Rc<Notification>` plus LVGL callback registration; optional map/canvas resource slots exist on every `Entry` |
| Events | Native queue initial capacity 10 is not a hard bound; key is recorded natively but not returned by the current guest pop calls |
| Images | LV_USE_IMAGE is enabled, but surveyed LVGL FS adapters and optional PNG/JPEG/GIF decoders are disabled; an image constructor alone would not provide VFS-backed loading |
| Fonts/theme | Guests can select six host ROM-backed semantic font roles and four theme surfaces; broad property/theme coverage is still incomplete |

The build-time `GraphicsRegistry` filters guest Rust/C wrappers and generates host `Function` metadata from the same selected-name set. It emits `graphics_abi_inventory.json` into Cargo's `OUT_DIR`; this is a build artifact, not linked runtime metadata. The current inventory contains 1,084 discovered calls, with 129 guest-callable/host-dispatched and 955 excluded. It records signatures, IDs, adapter classes, ownership and unsupported reasons. Generated scalar geometry dispatch requires explicit object-handle and signed-scalar policies; no guest word is forwarded as a native pointer. Grid descriptors, Calendar records, Chart series slots, Line points, List strings, and Tileview navigation use named custom adapters. The `xila_graphics_call` transport and legacy IDs remain unchanged. ABI capability/version negotiation is still outstanding.

The existing adapter already copies guest strings, button maps and indexed-canvas data into host-owned storage, validates guest memory spans, and gates LVGL work under the graphics-manager lock. These are invariants to preserve, not evidence that all widget/resource paths are complete.

### Enabled widget coverage

| LVGL family | Current host coverage | Remaining vertical slice |
| --- | --- | --- |
| Button | Constructor only | label/child behavior, state and click/value events |
| Buttonmatrix | Constructor, copied map, selected index | button control/width/text queries and selected event |
| Checkbox | Constructor, text | checked state/query and value event |
| Label | Constructor, copied text | long mode, alignment, wrap/selection and text queries |
| Tabview | Constructor, add tab, active-index query | active-tab setter, tab handles and change events |
| Textarea | Constructor, copied text, one-line/max-length, text output | placeholder, cursor/selection, editing options/events |
| Keyboard | Constructor, textarea attachment | mode, button map/state and key event payload |
| Arc | Constructor, range/value/mode | angle/background/rotation APIs and value-change tests |
| Bar | Constructor, range/value/start-value/mode/orientation | orientation/value events and modes verified against app use |
| Slider | Constructor, range/value/start-value/mode/orientation | orientation/value events and range mode tests |
| Switch | Constructor, orientation; generic checked state | toggle convenience/query and value-event tests |
| Spinner | Constructor, animation parameters, duration/sweep queries | lifecycle/event tests |
| Dropdown, Roller | Constructors, bounded copied options, selection/count/highlight/open state and selected text | per-widget event integration and additional navigation/query operations |
| LED | Constructor, bounded brightness setter, on/off/toggle operations | color property and guest brightness query |
| Calendar | Constructor, today/shown date, copied highlighted dates, shown-date query | pressed-date selection query/events, day-name/header configuration |
| Spinbox | Constructor, validated range/value/step/digit format, rollover, increment/decrement | cursor/digit-direction controls and keyboard integration |
| Chart | Constructor, type, point count, axis ranges, bounded series slots/value set/query | events, update mode, multi-axis details and interaction queries |
| Line | Constructor, copied/retained points and y-inversion | styling helpers, point query and lifecycle runtime test |
| List | Constructor, bounded text sections/buttons, safe child handles, button text update | selection/event integration, icons and query operations |
| Tileview | Constructor, bounded tile creation, active-tile navigation/query | selection events and active row/column query |
| Table, Menu, Msgbox | Not implemented | bounded cell/item handles, navigation and cleanup |
| Canvas, Image, Imagebutton, Animimg | IndexedCanvas Xila helper only; LVGL families not adapted | host-owned image/frame resources and explicit quotas/lifetimes |

This matrix distinguishes enabled LVGL code from practical guest usability. Constructor-only support does not satisfy the #483 acceptance criteria.

## Changes in the layout/style tranches

Host adapters support position, size, x/y/width/height queries, object alignment, layout update, flex alignment, and selecting only LVGL's built-in default/flex/grid layout IDs. Grid track arrays are copied, sentinel-checked, limited to 31 tracks per axis and 512 retained descriptors per VM, then kept alive until layout replacement or native deletion completes. Grid cell positions/spans and alignment selectors are validated. The `grid_fr` helper encodes fractional tracks through append-only Xila ID `0xf006` rather than adding `lv_grid_` to the alphabetically numbered legacy LVGL catalog. Signed coordinates and dimensions are checked for valid memory32/memory64 encodings before LVGL calls; geometry remains handle-based and returns scalars rather than native structs or pointers.

The current style slice adds RGB888 local background/text/border colors, background opacity, border side/width, radius, text alignment, min/max dimensions, semantic theme surfaces and six host-owned font IDs. Style part/state selectors are restricted to known LVGL values. No guest-native style or font pointer is accepted. The range-control slice adds bounded range/value/mode/orientation operations for Arc/Bar/Slider, Switch orientation/state operations, and Spinner animation configuration.

Dropdown/Roller receive newline-separated guest options synchronously; LVGL's dynamic APIs copy them, so guest pointers are not retained. Inputs are capped at 1,024 bytes and 64 options per object, with an 8,192-byte per-VM high-water budget (conservative on replacements because LVGL setters return no allocation status). Infinite Roller mode is limited to 512 input bytes to bound LVGL's repeated option buffer.

## Reviewable PR roadmap

1. **Fundamental geometry/layout (in progress):** expose safe position/size/align and flex/grid operations; add host/guest ABI tests and document the API.
2. **ABI manifest and inventory:** derive guest exposure and host dispatch metadata from one declared support list; generate deterministic per-symbol classifications and compatibility fingerprint/capability inquiry while preserving legacy IDs.
3. **Theme, styles and fonts:** validated local style properties and semantic theme/font IDs; no raw `lv_style_t`/`lv_font_t` pointers.
4. **Object registry:** per-VM `BTreeMap<u16, Entry>`, monotonic IDs 1..=65535, independent live-object quota, targeted post-delete reclamation, no global reaping or reverse lookup scans; no slab/generational handles.
5. **Events and controls:** versioned bounded event payload with key/target identity, followed by complete vertical slices for common controls and existing partial widgets.
6. **Advanced resources/widgets:** separately scoped bounded list/table/menu/chart/image and VFS-backed asset ownership work.
7. **Integration:** standalone WASM UI acceptance, migration/limits docs, native/browser tests and available MCU checks.

Tasks 1–3 are separable from the registry data structure for scalar handle calls, but new resource-retaining APIs should wait for the ownership/cleanup contract. The event work depends on the registry's stable identity lifecycle.

## Validation status and limitations

- `cargo +stable test -p xila-graphics-bindgen` passed (6 tests).
- Host and memory64 `cargo check -p xila-wasm ... --tests` passed for the geometry/layout change.
- Native `graphics_core` test execution currently fails at link time because the local LVGL archive does not define Xila's generated font symbols (`font_montserrat_*`, `font_unscii_*`). The documented x86_64 test target is also not installed; this environment's host target is aarch64. Thus the new runtime integration assertions have been type-checked but not executed here.
- No ESP32-S3 build, allocation profile, fragmentation benchmark, lock timing, browser app acceptance, or peak-RAM measurement has been run. No performance/MCU cost estimate is asserted as a measurement.
- PRs and child issues are to be opened per tranche; #483 must remain open until its acceptance criteria are satisfied.
