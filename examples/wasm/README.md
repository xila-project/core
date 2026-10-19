# Browser example

From the repository root, prepare the separate application disk and build the
browser runtime:

```sh
cargo make prepare-wasm-image
cargo make generate-fonts
cargo check -p wasm_example --target wasm32-unknown-unknown
cd examples/wasm
RUSTUP_TOOLCHAIN=stable trunk serve
```

Trunk copies `target/seed/wasm/seed.img` beside the runtime. Building the Rust
runtime alone requires no prebuilt guest binaries; serving a fresh installation
requires this image. Build guests and images with the same
`EMBEDDED_I18N_LOCALE` as the runtime (`en`, `fr`, `es`, `de`, `pt-BR` or `it`).

Use localhost or HTTPS in a browser supporting OPFS,
`FileSystemFileHandle.createWritable`, and Web Locks.

## Running guest WebAssembly applications

Calculator, Clock, Pixel Painter, Text Editor and Synth are provisioned in `seed.img` at build time,
along with their shortcuts. The first startup streams this image into the disk
cache only when the selected OPFS file is absent or zero bytes. It validates and
mounts LittleFS before account initialization and the first persistent commit.
Failures leave a new disk retryable. Existing nonempty disks are mounted without
downloading a seed, copying applications or running their installers. Mount errors
never authorize erasing a disk. Publishing a newer image does not upgrade existing
installations; choose a new disk to try a fresh bundle while retaining the old one.

Clock's settings directory is provisioned at `/configuration/shared/clock` and
owned by the seed's administrator account (UID 1000) on a new installation. Its saved preferences and
alarm follow the OPFS persistence limits below. Other accounts need write access
to this directory to save settings. Clock alerts require the application to stay
open; browser background-tab throttling can delay them.

On a fresh disk, log in as **administrator** with an empty password; otherwise
use your saved credentials. Open the application drawer using the Xila button
in the dock. Select
**Calculator**, **Clock**, **Pixel Painter**, **Text Editor** or **Synth**. Text Editor uses a temporary on-disk working copy and edits UTF-8-safe 4 KiB pages. They can also be launched from Xila's terminal:

```text
/binaries/wasm /binaries/calculator
/binaries/wasm /binaries/clock
/binaries/wasm /binaries/pixel_painter
/binaries/wasm /binaries/text_editor
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
  limits below. Other guest files must be present before launching; there is no
  upload mechanism.
- Click **Enable audio** in the browser page before launching Synth. The click
  starts one `WebAudioDevice` and mounts it at `/devices/audio/playback`; Synth
  then writes ordinary PCM through its file descriptor. If Synth is already
  open, choose **Retry audio** inside its window. Browsers may require this
  gesture because audio contexts cannot start during page initialization.
- Applications are distributed in a separate seed image. The disk cache still
  occupies 16 MiB; running a guest also needs Wasmi's module and guest memory.
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

The storage regression suite also exercises OPFS persistence, first-use seed
import through Fetch, zero-byte placeholders, failed imports, corrupt existing
disks, lease handling and reopening an existing disk without downloading a seed.
The image-builder tests remount populated disks and check reproducibility.

The probes are not included in normal example startup. Generated
guest binaries are not committed. This is focused browser validation, not a
claim of full WASI/graphics compatibility or support across all browsers.

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
given disk at a time. A new disk imports the reference image; existing nonempty
disks do not depend on it being available.

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

See [Synth](../../executables/synth/README.md) for controls, silent mode,
existing-disk behavior and the audio smoke-test procedure.
