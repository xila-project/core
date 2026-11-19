# Logo renderer comparison

The baseline is Core `bf626487a548a3567a7f10c52344ca1612facb60`. Each case was captured with the existing LVGL snapshot API over an opaque background, with the display and snapshot buffer set to the format being tested. The dark and light colors are zinc `#18181b` and `#f4f4f5`, swapped between background and foreground. The device-independent PNGs preserve the rendered RGB values after format decoding. RGB565 is expanded to 8-bit channels by bit replication.

| Style and background | RGB565 | XRGB8888 |
| --- | --- | --- |
| Filled on light | [Sheet](filled-light-rgb565.png) | [Sheet](filled-light-xrgb8888.png) |
| Filled on dark | [Sheet](filled-dark-rgb565.png) | [Sheet](filled-dark-xrgb8888.png) |
| Outline on light | [Sheet](outline-light-rgb565.png) | [Sheet](outline-light-xrgb8888.png) |
| Outline on dark | [Sheet](outline-dark-rgb565.png) | [Sheet](outline-dark-xrgb8888.png) |

Each row in a sheet shows the rasterizer, the primitives, and the absolute channel difference multiplied by four and clipped to 255. The 32 and 64 px rows are doubled with nearest-neighbor scaling. All 96 [native-size PNGs](images/) are available separately.

## Fidelity

Each range below spans the two backgrounds and two formats. Maximum and mean are absolute 8-bit RGB channel differences. A pixel exceeds a threshold when any RGB channel exceeds it. [Per-case metrics](pixels.csv)

| Size | Style | Max | Mean | Pixels >8 | Pixels >16 | Pixels >32 |
| ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 32 | Filled | 98–99 | 5.79–6.27 | 10.94–12.50% | 9.77–10.16% | 7.42–8.20% |
| 32 | Outline | 207–214 | 10.42–10.76 | 17.58–19.53% | 16.41–16.80% | 14.06–14.84% |
| 64 | Filled | 90–91 | 2.13–2.21 | 5.00–5.57% | 4.59–4.76% | 3.32–3.91% |
| 64 | Outline | 199–206 | 3.34–3.52 | 8.35–8.57% | 7.18–7.69% | 4.44–5.03% |
| 128 | Filled | 104–108 | 1.76–1.83 | 3.17% | 2.93–3.03% | 2.20–2.54% |
| 128 | Outline | 202–214 | 2.74–2.88 | 5.02–5.07% | 4.64–4.74% | 3.71–4.08% |
| 160 | Filled | 120–124 | 1.17–1.20 | 2.35–2.44% | 2.23–2.27% | 1.73–1.95% |
| 160 | Outline | 207–214 | 1.87–1.96 | 3.70–3.84% | 3.44–3.47% | 2.64–3.07% |

The 32 px outline shows the largest visual change. Integer primitive vertices shift diagonal coverage and the LVGL one-pixel stroke has a different coverage profile from the rasterizer's four-neighbor erosion. The clipped rounded rectangles improve the outer right-angle corners. The other curved portions use two chords per Bézier, with some chords collapsing at the smallest sizes. The snapped center separations stay open.

## Heap and redraw time

Persistent bytes are the net change from creating, positioning, and laying out a logo before its first draw. The benchmark uses a fixed color and the resting pose. Cold peak includes first-use LVGL circle-mask cache entries. Warm peak measures the next redraw above its own entry baseline. Peaks include draw tasks and masks for the snapshot, with no retained framebuffer allocation in the measurement window. The background-only peak was 240 B in every case. The cold snapshot left 0–20 B in the rasterizer and 20–114 B in the prototype, and every warm snapshot left 0 B. These are x86_64 allocator bytes. [Baseline counters](baseline.csv) · [Prototype counters](primitives.csv)

Time is the median across four background/format combinations, after subtracting each case's background-only time. Each time uses the median of nine batches of 128 snapshots on an Intel Core i7-1370P, Linux x86_64, with a Rust 1.97.0 release build. This includes LVGL traversal and software rendering. It serves as a host proxy for redraw cost.

| Size | Style | Persistent B old → new | Cold peak B old → new | Warm peak B old → new | Redraw µs old → new |
| ---: | --- | ---: | ---: | ---: | ---: |
| 32 | Filled | 3,152 → 318 | 560 → 524 | 560 → 504 | 6.37 → 13.35 |
| 32 | Outline | 3,161 → 318 | 560 → 500 | 560 → 480 | 6.46 → 13.92 |
| 64 | Filled | 5,616 → 318 | 560 → 536 | 560 → 504 | 8.02 → 20.01 |
| 64 | Outline | 5,625 → 318 | 560 → 538 | 560 → 480 | 7.92 → 20.27 |
| 128 | Filled | 15,696 → 318 | 560 → 548 | 560 → 504 | 13.86 → 30.57 |
| 128 | Outline | 15,705 → 318 | 560 → 562 | 560 → 480 | 12.79 → 28.26 |
| 160 | Filled | 23,568 → 318 | 560 → 554 | 560 → 504 | 17.74 → 38.47 |
| 160 | Outline | 23,577 → 318 | 560 → 574 | 560 → 480 | 16.55 → 33.14 |

## Thumb build

A locked Rust 1.97.0 release build of `xila-graphics` for `thumbv7em-none-eabihf` with `rendering_rgb565` gives the following archive section totals. Firmware link-time garbage collection and hardware redraw timing are outside this measurement.

| `libgraphics.rlib` sections | Rasterizer | Primitives | Delta |
| --- | ---: | ---: | ---: |
| `.text` and `.text.*` | 84,460 B | 84,426 B | −34 B |
| `.rodata` and `.rodata.*` | 590,561 B | 590,245 B | −316 B |

## Reproduce

Use Core checkouts at the baseline and prototype revisions. Copy `subsystems/graphics/tests/logo_evidence.rs` from the prototype into the baseline checkout. The harness calls the same logo API and reads `MemoryManager::thread_allocations()` on the draw thread. The peak sampler wraps that allocator and samples after each allocation while enabled. The buffer and display are allocated before sampling.

Run in each checkout with a separate in-checkout target directory. Set `XILA_LOGO_EVIDENCE` to the desired raw output path.

```sh
export CARGO_TARGET_DIR="$PWD/target"
export TMPDIR="$CARGO_TARGET_DIR/scratch"
mkdir -p "$TMPDIR"
mise run deps:npm
XILA_LOGO_EVIDENCE="$CARGO_TARGET_DIR/logo-capture" \
  cargo +1.97.0 test --locked --release --target x86_64-unknown-linux-gnu \
  -p xila-graphics --features rendering_xrgb8888 --test logo_evidence -- --ignored

cargo +1.97.0 build --locked --release -p xila-graphics \
  --target thumbv7em-none-eabihf --features rendering_rgb565
arm-none-eabi-size -A "$CARGO_TARGET_DIR/thumbv7em-none-eabihf/release/libgraphics.rlib" |
  awk '$1 ~ /^\.text(\.|$)/ {text += $2} $1 ~ /^\.rodata(\.|$)/ {rodata += $2}
       END {print text, rodata}'
```

Use Python 3 with Pillow to regenerate the images and pixel metrics. Copy the `measurements.csv` from each run to `baseline.csv` and `primitives.csv`.

```sh
python3 test-evidence/logo-primitives/compare.py \
  /path/to/baseline/capture /path/to/prototype/capture test-evidence/logo-primitives
```

## Recommendation

Review the filled logo and the actual-size 128 px watermark before adopting the primitive renderer. If the current contour is the fidelity target, keep the single object and use LVGL for opaque interiors and snapped straight runs. Preserve the rasterizer's coverage only along diagonals and curves, including the outline erosion. One option for common factors is to store sparse edge runs in flash and reuse them across the four arms. A renderer for arbitrary factors could blend a bounded row or tile at draw time. A corner-only mask would leave the visible diagonal differences.
