# Xila Memory Subsystem (`xila-memory`)

`xila-memory` defines the portable `no_std` memory management interface ([`ManagerTrait`](./src/trait.rs)), global allocator bridge ([`Manager`](./src/manager.rs)), capability bitflags ([`CapabilityFlags`](./src/capability.rs)), and a static multi-region allocator ([`RegionManager`](./src/region_manager.rs)) for platforms without a vendor allocator.

## Capability Semantics

Callers never request memory by physical location (such as "internal" or "external"). Callers request **capabilities** in [`CapabilityFlags`](./src/capability.rs) (`u8`), and each platform backend maps those flags onto its physical memory regions and hardware constraints:

| Flag | Bit | Meaning |
|---|---|---|
| `Executable` | `1 << 0` | Memory can be executed as machine instructions (JIT/Wasm code buffers). |
| `DirectMemoryAccess` | `1 << 1` | Memory is reachable by platform DMA controllers and satisfies the region's alignment rules. |
| `AlwaysAccessible` | `1 << 2` | Memory remains CPU-accessible while external flash instruction/data caches are disabled (e.g., ISRs or flash-safe paths running during internal flash erase/program operations). |
| `empty()` (`0`) | — | No special hardware constraints; the platform places the allocation according to its default placement policy. |

### Why `AlwaysAccessible`?

Espressif PSRAM depends on the external-memory cache. An ISR or flash-safe
path must use storage that remains accessible during flash erase and program
operations. On the supported boards, reserve the primary heap from internal
data RAM that remains accessible during those operations. ESP32-P4 has a
cached internal data path, which affects DMA alignment. Flash cache suspension
still allows accesses to internal data RAM used by IRAM-safe interrupt handlers.
See Espressif's [flash concurrency guide][flash-concurrency] and [external RAM
restrictions][external-ram].

Use the flag when allocating ISR state, DMA buffers or descriptors used by an
ISR, and flash staging buffers that will actually be accessed with the cache
suspended. Allocate them before entering the cache-off path. The flag covers the
allocation's storage. The handler, its callees, its stack and all transitively
accessed data must also be available in that path.

### DMA Alignment Without Per-Allocation Headers

`ManagerTrait::deallocate` receives `(pointer, layout)` without `CapabilityFlags` and `RegionManager` stores zero per-allocation headers. Silently rounding up `layout.size()` or `layout.align()` inside `allocate` would cause the region heap's `deallocate` to recompute a different block size and corrupt free-list accounting—and on cached targets, sharing a cache line between a DMA buffer and adjacent heap metadata causes data corruption during cache invalidation.

Instead, each [`RegionDescriptor`](./src/region_manager.rs) encodes a power-of-two DMA alignment in one byte. When `CapabilityFlags::DirectMemoryAccess` is requested, a region qualifies only if:
- its `CapabilityFlags` contain `DirectMemoryAccess`, and
- `layout.align() >= dma_alignment` and `layout.size() % dma_alignment == 0`.

Unaligned DMA requests naturally skip high-alignment cached regions (such as PSRAM) and fall back to lower-alignment regions (such as internal SRAM), or return `None` if no capable region matches.

## Platform Mapping

The Espressif mapping follows [esp-metadata-generated 0.5.3][metadata]
(`soc_has_psram`, `dma_can_access_psram` and `soc_internal_memory_cached`)
and [esp-hal 1.2.2's region alignment checks][region-alignment]. The 4-byte
minimum for uncached SRAM covers word-aligned burst DMA. ESP32-S3 PSRAM uses
the largest configurable [S3][s3-cache] data-cache line size, and C5/C61 PSRAM
the largest (64-byte) external burst, since inbound PSRAM transfers must be
aligned to the burst size. ESP32-S2 PSRAM is not advertised as DMA-capable:
only SPI2, I2S0 and the crypto DMA can reach it ([S2][s2-cache]), and a
capability flag cannot name the channel that will use a buffer. The P4 limits cover its 64-byte internal line and up to 128-byte
external line. DMA reachability and burst alignment still depend on the
[channel and transfer configuration][dma-engine]. On capable chips, PSRAM
may work with some channels only. Callers that also require internal data
RAM during cache-off operations can combine `DirectMemoryAccess` and
`AlwaysAccessible`.

| Platform / Target | Region `0` (Primary / Fast) | Region `1` (Secondary / Large) | `Executable` |
|---|---|---|---|
| **ESP32** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` (`dma_alignment = 4`) | Optional PSRAM: `empty()` (esp-hal DMA excludes PSRAM) | Unsupported (`None`) |
| **ESP32-S2** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` (`dma_alignment = 4`) | Optional PSRAM: `empty()` (only SPI2, I2S0 and crypto DMA can reach it) | Unsupported (`None`) |
| **ESP32-S3, ESP32-S31** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` (`dma_alignment = 4`) | Optional PSRAM: `DirectMemoryAccess` (`dma_alignment = 64`) | Unsupported (`None`) |
| **ESP32-C5, ESP32-C61** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` (`dma_alignment = 4`) | Optional PSRAM: `DirectMemoryAccess` (`dma_alignment = 64`, the largest external burst) | Unsupported (`None`) |
| **ESP32-P4** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` (`dma_alignment = 64`, L1 DCache) | Optional PSRAM: `DirectMemoryAccess` (`dma_alignment = 128`, L2 cache) | Unsupported (`None`) |
| **ESP32-C2, C3, C6, H2** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` (`dma_alignment = 4`) | None (`REGION_COUNT = 1`, no PSRAM support in esp-hal) | Unsupported (`None`) |
| **Nordic (nRF52840)** | Internal SRAM: `DirectMemoryAccess \| AlwaysAccessible` | None (single region) | Unsupported (`None`) |
| **Host (`std`)** | Anonymous `mmap` (`PROT_READ \| PROT_WRITE`): `AlwaysAccessible` | Anonymous `mmap` (`PROT_READ \| PROT_WRITE \| PROT_EXEC`): `Executable \| AlwaysAccessible` | Region `1` |
| **Browser (`wasm`)** | Linear memory (`memory_grow`): `AlwaysAccessible` | None (single region) | Unsupported (`None`) |

## Default Placement Policy and Fallback

Placement policy is **platform behavior** configured via [`RegionManager::new`](./src/region_manager.rs), not part of the caller-visible allocation API:

- Regions are ordered from highest-priority / fastest (`0`) to lowest-priority / largest (`N - 1`).
- With a `large_allocation_threshold` (e.g. `4096` bytes on Espressif):
  - Allocations strictly smaller than the threshold probe regions forward (`0..N`), placing small general-purpose objects in fast internal RAM first.
  - Allocations at or above the threshold probe regions in reverse (`(0..N).rev()`), steering large buffers into external PSRAM first when available.
- **Fallback:** If the preferred region is uninitialized (e.g. a board without PSRAM), lacks a requested capability (`AlwaysAccessible`, `DirectMemoryAccess`), rejects the caller's DMA alignment, or is full, the manager continues across the remaining capable regions in order before returning `None`.
- **Reallocation** preserves the requested capability flags and resizes in place when possible.
  Rust callers pass the original flags to `Manager::reallocate` or `ManagerTrait::reallocate`.
  The C allocator glue (`xila-c-support`) stores the flags in the existing one-word allocation header, leaving 25 size bits
  on 32-bit targets for the layout including its header and alignment padding.
  - A **shrink** keeps the pointer when the new layout still satisfies the original region's
    alignment and DMA requirements. The freed tail returns to that region.
  - A **growth below the threshold** tries the original region first, in place or elsewhere in
    the region. If it cannot serve the new layout, the other eligible regions follow in forward order.
  - A **growth at or above the threshold** tries eligible regions in reverse order. Ordinary buffers
    can still move to PSRAM. The old region attempts an in-place resize when it is reached.
  - A destination must provide the requested capabilities and satisfy its own DMA alignment and
    size requirements. Failure leaves the original allocation valid.
- Deallocation finds the owning region by its address range.

Boards without initialized PSRAM continue to place ordinary allocations in
their primary region. The host keeps its existing lazy mappings and growth
policy. The C allocator glue keeps the same `uint8_t` capability type and existing bits
(`MEMORY_CAPABILITIES_ALWAYS_ACCESSIBLE` is bit 2).

## Overhead Budget

`RegionManager<N>` uses static `[Region; N]` storage backed by an address-ordered first-fit free list ([`heap.rs`](./src/heap.rs); free holes live in the free memory itself, and the list is kept reachable so a block can be resized in place) under `critical_section::Mutex`, with **zero per-allocation headers** and **zero heap allocation** for its own bookkeeping. A region costs 20 B on 32-bit targets (a 16 B heap and the descriptor).

Blocks and holes are multiples of two words (8 B on 32-bit targets, 16 B on 64-bit ones), the size of a hole header: whatever lies between two blocks is then empty or a whole hole, so no resize can strand a word that is neither in a block nor in a hole. The price is rounding: a block of `n` bytes occupies `n` rounded up to 8 B (16 B), and at least 8 B (16 B). Over sizes uniformly spread in `1..=4096` that is 3.5 B (7.5 B) of padding per block on average, 2 B (4 B) more than the previous word granularity; for sizes that are multiples of 4 B, as most Rust types are, the average goes from 0 B to 2 B on 32-bit targets.

The following pre-capability-reallocation measurements used a freestanding ELF probe built with Rust
`1.97.0 (2d8144b78 2026-07-07)`, `opt-level = "z"`, LTO, one codegen unit and
`panic = "abort"`. The probe initializes regions and exercises allocate,
reallocate, deallocate and statistics with runtime inputs read from fixed
addresses, so the manager is its only static. It uses stub critical-section
hooks. The toolchain's `llvm-size -A` gives the ELF section sizes. Heap backing
bytes and the firmware runtime are outside the measurement. RAM and `.text` come
from the same build of each row.

| Configuration | Target | Manager RAM | ELF `.text` | ELF `.data` / `.bss` |
|---|---|---:|---:|---:|
| `RegionManager<1>` (without `Debug`) | `thumbv7em-none-eabihf` | 28 B | 2,124 B | 28 B / 0 B |
| `RegionManager<1>` (without `Debug`) | `riscv32imac-unknown-none-elf` | 28 B | 1,910 B | 28 B / 0 B |
| `RegionManager<2>` (without `Debug`) | `thumbv7em-none-eabihf` | 48 B | 2,284 B | 48 B / 0 B |
| `RegionManager<2>` (without `Debug`) | `riscv32imac-unknown-none-elf` | 48 B | 2,114 B | 48 B / 0 B |
| `RegionManager<2>` (with `Debug`) | `thumbv7em-none-eabihf` | 60 B | 2,472 B | 60 B / 0 B |
| `RegionManager<2>` (with `Debug`) | `riscv32imac-unknown-none-elf` | 60 B | 2,354 B | 60 B / 0 B |
| `esp_alloc::EspHeap` (0.11.0, LLFF) | `riscv32imac-unknown-none-elf` | 100 B | 1,896 B | 0 B / 100 B |

Each probe had 0 B of `.rodata`. The opt-in `Debug` feature adds three
32-bit statistics counters (12 B of static RAM) on these targets. These link sizes are probe-specific and omit
board startup, critical-section and cache maintenance costs.

The heap replaces `linked_list_allocator`: it drops the unused dummy hole size
and the pending-extend byte, so a region is 8 B smaller (the same probe on the
previous heap: 36 B for `RegionManager<1>`, 64 B for `RegionManager<2>` and 76 B with
`Debug`). In-place
reallocation and the owned heap add flash. On the previous heap the same probe
measures `.text` at 2,030 B (`thumbv7em-none-eabihf`) and 1,894 B
(`riscv32imac-unknown-none-elf`) for `RegionManager<1>`, 2,044 B and 1,980 B for
`RegionManager<2>` without `Debug`, and 2,184 B and 2,140 B with it. The price
is 240 B (thumb) and 134 B (RISC-V) of flash for the two-region manager without
`Debug`.

[metadata]: https://docs.rs/crate/esp-metadata-generated/0.5.3/source/src/_build_script_utils.rs
[region-alignment]: https://docs.rs/crate/esp-hal/1.2.2/source/src/dma/aligned.rs
[s2-cache]: https://docs.rs/crate/esp-hal/1.2.2/source/src/soc/esp32s2/mod.rs
[s3-cache]: https://docs.rs/crate/esp-hal/1.2.2/source/src/soc/esp32s3/mod.rs
[dma-engine]: https://docs.rs/crate/esp-hal/1.2.2/source/src/dma/
[flash-concurrency]: https://docs.espressif.com/projects/esp-idf/en/stable/esp32p4/api-reference/peripherals/spi_flash/spi_flash_concurrency.html
[external-ram]: https://docs.espressif.com/projects/esp-idf/en/stable/esp32p4/api-guides/external-ram.html
