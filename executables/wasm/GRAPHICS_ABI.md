# WASM graphics ABI audit and roadmap

This document tracks the guest-facing LVGL ABI, not the browser display backend. It records a source audit of the current checkout; counts are configuration-specific and are not runtime benchmarks.

## Current audit

| Surface | Current state |
| --- | --- |
| Enabled LVGL widget families | 28 surveyed in `subsystems/graphics/include/lv_conf.h` |
| Guest Rust wrappers emitted | Filtered to the 47 currently host-supported calls |
| Host-dispatched calls | 47; unsupported IDs return `GraphicsError::Unsupported` |
| Widget families with usable constructors | Button, Buttonmatrix, Checkbox, Label, Tabview, Textarea, Keyboard; Xila Window/Object/IndexedCanvas are extra abstractions |
| ABI IDs | Legacy calls are alphabetically numbered from discovered signatures; Xila extensions use `0xf000+` |
| Object registry | Per-VM `Vec<Entry>`, 256 lifetime-issued IDs, tombstones; full scans occur during reaping, reverse lookup and canvas accounting |
| Per-object adapter allocation | One `Rc<Notification>` plus LVGL callback registration; optional map/canvas resource slots exist on every `Entry` |
| Events | Native queue initial capacity 10 is not a hard bound; key is recorded natively but not returned by the current guest pop calls |
| Images | LV_USE_IMAGE is enabled, but surveyed LVGL FS adapters and optional PNG/JPEG/GIF decoders are disabled; an image constructor alone would not provide VFS-backed loading |
| Fonts/theme | Existing host semantic theme surfaces and ROM-backed font functions exist; guests do not yet receive safe semantic font IDs |

The build-time `GraphicsRegistry` filters guest Rust/C wrappers and generates host `Function` metadata from the same selected-name set. It emits `graphics_abi_inventory.json` into Cargo's `OUT_DIR`; this is a build artifact, not linked runtime metadata. The inventory classifies discovered calls, signatures, IDs, adapters, ownership and unsupported reasons. Initial registry testing over the full catalog found 1,074 discovered calls, with 92 supported and 982 excluded. Generated scalar geometry dispatch requires explicit object-handle and signed-scalar policies; no guest word is forwarded as a native pointer. The `xila_graphics_call` transport and legacy IDs remain unchanged. ABI capability/version negotiation is still outstanding.

The existing adapter already copies guest strings, button maps and indexed-canvas data into host-owned storage, validates guest memory spans, and gates LVGL work under the graphics-manager lock. These are invariants to preserve, not evidence that all widget/resource paths are complete.

## Changes in this tranche

Host adapters now support position, size, x/y/width/height queries, object alignment, layout update, flex alignment, and selecting only LVGL's built-in default/flex/grid layout IDs. Signed coordinates and dimensions are checked for valid memory32/memory64 encodings before LVGL calls; alignment and flex enum values are validated. Geometry remains handle-based and returns scalars rather than native structs or pointers.

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

- `cargo +stable test -p graphics_bindgen` passed (6 tests).
- Host and memory64 `cargo check -p wasm ... --tests` passed for the geometry/layout change.
- Native `graphics_core` test execution currently fails at link time because the local LVGL archive does not define Xila's generated font symbols (`font_montserrat_*`, `font_unscii_*`). The documented x86_64 test target is also not installed; this environment's host target is aarch64. Thus the new runtime integration assertions have been type-checked but not executed here.
- No ESP32-S3 build, allocation profile, fragmentation benchmark, lock timing, browser app acceptance, or peak-RAM measurement has been run. No performance/MCU cost estimate is asserted as a measurement.
- PRs and child issues are to be opened per tranche; #483 must remain open until its acceptance criteria are satisfied.
