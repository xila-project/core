# Browser example

From the repository root, build the bundled guests first, generate the LVGL
fonts, and then build the browser example:

```sh
cargo +stable build -p calculator -p clock -p pixel_painter -p synth --target wasm32-wasip1 --release
cargo make generate-fonts
cargo +stable build -p wasm_example --target wasm32-unknown-unknown
```

To serve it in a browser, install [Trunk](https://trunkrs.dev/) and run:

```sh
cd examples/wasm
RUSTUP_TOOLCHAIN=stable trunk serve
```

Use localhost or HTTPS in a browser supporting OPFS,
`FileSystemFileHandle.createWritable`, and Web Locks.

## Running guest WebAssembly applications

Calculator, Clock, Pixel Painter, and Synth are embedded from
`target/wasm32-wasip1/release/` at browser build time. Startup copies them to
`/binaries/` and runs their installers to create application-drawer shortcuts.
Rebuild the guests before rebuilding the browser example whenever an
application changes. Guest builds must place these artifacts in the repository's
default `target` directory.

Clock's settings directory is provisioned at `/configuration/shared/clock` and
assigned to the administrator account when it exists. Its saved preferences and
alarm follow the OPFS persistence limits below. Other accounts need write access
to this directory to save settings. Clock alerts require the application to stay
open; browser background-tab throttling can delay them.

On a fresh disk, log in as **administrator** with an empty password; otherwise
use your saved credentials. Open the application drawer using the Xila button
in the dock. Select
**Calculator**, **Clock**, **Pixel Painter**, or **Synth**. They can also be
launched from Xila's terminal:

```text
/binaries/wasm /binaries/calculator
/binaries/wasm /binaries/clock
/binaries/wasm /binaries/pixel_painter
/binaries/wasm /binaries/synth
```

The example mounts the Wasmi-backed runtime at `/binaries/wasm`, with the same
graphics bindings as the native example. For other guests that are already
present in Xila's virtual filesystem:

```text
/binaries/wasm /path/to/guest.wasm
```

WASI Preview 1 command guests should export `_start: () -> ()`. Use
`/binaries/wasm --install /path/to/guest.wasm` to call `__install: () -> ()`
instead. Graphical guests must use compatible Xila graphics bindings; see the
[runtime documentation](../../executables/wasm/README.md) for supported APIs and
limits.

## Visual identity

The browser help panel uses local Montserrat normal and italic variable web fonts, following the shared 400/600/700 text, heading and emphasis hierarchy. Trunk copies `fonts/` alongside the example; font rendering needs no Google Fonts connection. Keep `fonts/OFL.txt` with those files. The font and logo source revision is recorded in `fonts/README.md`.

The OS canvas still uses the existing localized LVGL Montserrat Regular subsets at 10/14/18/28px, and Unscii for terminal content. Web font files are not compiled into the OS font tables or loaded into MCU flash. Bold wordmarks are outlined SVG assets.

## Browser limitations

- The example persists its filesystem in OPFS; see the storage and durability
  limits below. Startup reinstalls the bundled Calculator, Clock, Pixel Painter,
  and Synth. Other guest files must be present before launching; there is no
  upload mechanism.
- Synth writes ordinary PCM to `/devices/audio/playback`. Browser playback still
  requires the host to start `WebAudioDevice` from a user gesture and mount it at
  that path before Synth starts; this integration is handled by the dependent
  browser gesture PR.
- Embedding applications increases the browser WASM download and retains the
  embedded bytes alongside the VFS copy. Running it also needs Wasmi's module
  and guest memory. This simple approach favors convenience over RAM usage.
- Guests run cooperatively in Wasmi on the browser's executor, not as separate
  native browser WebAssembly instances. Fuel exhaustion and supported WASI
  suspension calls yield to the executor.
- There is no Xila network manager in this example. The browser HTTP device
  uses Fetch and remains subject to browser security restrictions, including
  CORS; it does not provide raw sockets.
- Runtime limitations still apply, including the supported graphics subset,
  clock-only WASI polling, unsupported suspension in WebAssembly start sections,
  and the currently unused stack-size option.

Per-call WASI diagnostics are logged at Trace level, which the browser logger
does not display. Informational messages, warnings, and errors remain visible.

## Browser verification

Verified in headless Chromium 154 with a Trunk development build using Rust
stable. Temporary startup probes loaded guest files into the example's VFS and
launched them through the mounted `/binaries/wasm` executable:

- A minimal guest executed a host call, exhausted and replenished its fuel,
  resumed after WASI `sched_yield`, and handled success and nonzero `proc_exit`.
- The real `calculator` guest, built for `wasm32-wasip1`, evaluated `1 + 2 = 3`
  through LVGL button events, used WASI clock polling, and exited successfully
  with its window removed after a close request.

Calculator bundling was additionally checked with a temporary probe that read
the startup-created guest and shortcut without creating either, then ran the
bundled guest through the same evaluation and teardown checks.

The probes are not included in normal example startup; bundled applications are.
Generated guest binaries are not committed. This is focused browser validation,
not a claim of full WASI/graphics compatibility or support across all browsers.

## Persistent storage

The example stores each 16 MiB MBR/LittleFS disk as an origin-private file.
Open the left Help panel to select a disk, create a date-named disk, or remove
an unused disk. New images are named `xila_YYYY-MM-DD.img`; repeated creations
on the same date receive a numeric suffix. The existing `xila_drive.img` remains
available as the default disk, preserving data from earlier versions. The
selected disk is remembered for this browser origin.

Switching disks stops new writes, commits pending changes, releases the current
disk's Web Lock, and reloads Xila on the selected image. A disk open in another
tab cannot be removed. Removing a disk asks for confirmation and permanently
deletes its files. The default administrator with an empty password is created
only on a fresh disk. Existing users/groups are loaded even if the administrator
account was removed. Invalid MBRs or filesystem mount errors stop boot without
reformatting an existing image. A Web Lock allows only one Xila tab to open a
given disk at a time.

Synchronous OPFS handles are available only in workers, while this example's
UI and filesystem run on the browser main thread. The drive therefore keeps a
16 MiB memory cache and asynchronously commits an atomic disk snapshot at boot,
every second when dirty, and on normal shutdown. Saving temporarily needs
another 16 MiB snapshot, plus browser-internal buffering. There is no growing
queue of disk snapshots. Clean disks are not rewritten.

**Durability:** synchronous filesystem flushes update the cache, not OPFS.
`DriveDevice::sync().await` waits for an OPFS commit and reports storage errors;
autosave errors are logged and retried. Abrupt reloads, tab closure, or crashes
can lose changes since the last successful save. Background-tab timer throttling
can increase that window. OPFS is browser-managed storage: quota limits, private
browsing, clearing site data, or browser eviction can remove it. This is not a
substitute for backups. Changing the disk size does not silently resize an
existing image.

To reset the demo, clear its site data in browser settings. Alternatively, close
all Xila tabs and run this on a separate page on the same origin in the console:

```javascript
const root = await navigator.storage.getDirectory();
await root.removeEntry("xila_drive.img");
```

## Browser storage tests

Install `wasm-bindgen-cli` matching the resolved `wasm-bindgen` dependency and a
Chrome/Chromium WebDriver matching your browser, then run from the repository root:

```sh
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
CHROMEDRIVER=/path/to/chromedriver \
cargo test -p drivers_wasm --target wasm32-unknown-unknown --test opfs
```

For a non-default browser binary, set `WASM_BINDGEN_TEST_WEBDRIVER_JSON` to a JSON
file containing `{"goog:chromeOptions":{"binary":"/path/to/chromium"}}`.
These tests use real OPFS files, including MBR/LittleFS save/reopen coverage.
The disk-name and collision rules can also be tested with
`node --test examples/wasm/tests/disk_manager.test.mjs`.
