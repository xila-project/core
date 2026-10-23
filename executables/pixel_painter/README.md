# Pixel Painter

A Wasm drawing app for Xila, built for `wasm32-wasip1` and launched through
`/applications/executables/wasm /applications/executables/pixel_painter`. It uses the checked guest graphics ABI
and WASI file I/O, and is bundled in the native and browser demonstrations.

Draw with a mouse or touch. The palette buttons are white (W, eraser), ink (K),
red (R), blue (B), green (G), yellow (Y), violet (V), and brown (Br). Choose a
1, 3, or 5 pixel square brush. Undo restores the previous stroke or Clear action.
The 160×100 logical picture scales to the available canvas with letterboxing;
resizing or rotating the display preserves the drawing.

Save/Open uses the absolute path in the text field. Focusing it opens an on-screen
keyboard; its accept/cancel key hides it. The default is
`/users/<user-id>/applications/pixel_painter/data/drawing.xpnt`, provisioned by
the launcher for each user. Enter a path inside an existing writable SD mount
to use removable storage; no particular SD mount path is assumed. Filesystem
availability and permissions determine whether a save is possible.

An optional first command-line argument supplies the initial path. XPNT version 1
files contain an eight-byte header (`XPNT`, version 1, width 160, height 100, reserved
0) followed by 16,000 palette indexes in row order. Opens reject wrong dimensions,
invalid palette indexes, truncated files, and trailing data before modifying the
current drawing. Reads are bounded to 16,009 bytes, even for larger inputs.

Open and Close each require a second attempt before discarding unsaved work.
Clear can be undone. Failed reads/writes leave the drawing in memory and show a
translated status message. Saving writes the chosen file directly: WASI currently
has no rename/sync adapter, so a failed write may leave that file incomplete.
Successful Save does not promise power-loss durability. The browser demo's OPFS
autosave/shutdown flow handles persistence of its virtual disk.

The guest stores 16 KB of pixels and a 16 KB undo snapshot, plus a temporary
16 KB file buffer during saves/opens. The host owns a separate 16 KB indexed image
and 1 KB palette. LVGL/widget/rendering allocations are additional. Uploads copy
guest data; no guest address survives an ABI call. The host caps canvas pixels
at 65,536 per VM and reclaims them after native object deletion or VM teardown.

## Build and test

Use the repository's configured development shell:

```sh
cargo test -p pixel_painter
cargo build -p pixel_painter --target wasm32-wasip1 --profile release-wasm
cargo clippy -p pixel_painter --target wasm32-wasip1 --all-targets -- -D warnings
cargo test -p wasm --features graphics,xila/graphics_rendering_xrgb8888 --test graphics_canvas --test pixel_painter
mise run run:cargo -- -p native_example
```

Smoke test: launch Pixel Painter from the application drawer, draw a fast diagonal,
switch colors and brush sizes, undo a stroke, Clear and Undo, Save, reopen, and
resize the window. Enter an invalid or read-only path and confirm the drawing
remains available. Reopen a saved picture after restarting the demo, and test
the unsaved Open/Close confirmations. The runtime test also executes the real
guest through Wasmi and checks pointer snapshots, Save/Open, Clear/Undo, corrupt
file handling, and window cleanup.

WT32-SC01 Plus validation remains part of #320, #321 (display), #322 (touch), and
#325 (SD). Host/Wasm checks do not establish physical-board correctness.
