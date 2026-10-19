# Reference filesystem images

`fs_image` creates a raw disk with the Xila MBR signature, a Xila partition and
LittleFS. It exclusively creates its output, remounts it, and verifies imported
contents and metadata. Existing output files are never replaced.

```sh
# Empty disk (16 MiB by default).
cargo run -p fs_image -- empty.img
# Import a directory; host ownership and timestamps are deliberately ignored.
cargo run -p fs_image -- populated.img 16777216 --root ./root
# Use prebuilt applications and the same shortcuts as their guest installers.
cargo run -p fs_image -- native.img 16777216 --bundle native --applications-dir target/wasm32-wasip1/release-wasm
cargo run -p fs_image -- browser.img 16777216 --bundle wasm --applications-dir target/wasm32-wasip1/release
```

Host-side population currently requires Linux or macOS. It opens source entries
relative to directory descriptors, rejecting symlink substitutions and FIFOs
even if the source tree changes during generation. Other native platforms can
consume the images published by CI.

Sizes must be multiples of 512 bytes and accommodate the MBR and LittleFS (at
least 1049600 bytes; populated bundles need more). Import accepts regular files
and directories with UTF-8 names, rejecting symlinks and special files. It uses
sorted traversal, zero timestamps, root ownership, directory/binary mode 0755
and other file mode 0644. Account directories are 0700; Clock and Pixel Painter
configuration directories belong to UID/GID 1000. Repeating a build with identical
inputs and tool versions produces identical bytes; host file permissions and
modification times do not enter the image.

The native bundle contains Calculator, Clock, Pixel Painter, Synth and Weather. The
browser bundle contains Calculator, Clock, Pixel Painter and Synth. Both include native application
shortcuts and configuration directories. Set `EMBEDDED_I18N_LOCALE=en`, `fr`, `es`, `de`, `pt-BR`, `it` or `pl`
while building both guests and images. No passwords or random salts are included.
An explicit pending marker allows runtime account initialization to finish after
an interruption; existing saved credentials are preserved.

`cargo make prepare-native-image` builds release-wasm guests and writes
`target/seed/native/seed.img`. `cargo make prepare-wasm-image` builds release
guests and writes `target/seed/wasm/seed.img`. These recipes replace only their
generated reference outputs, never a live runtime disk. Generated images are
ignored by Git. CI publishes localized reference images as artifacts, and serves
the matching browser image beside each localized runtime.

An image update does not upgrade existing installations. Application updates
are a separate workflow. The raw image can also be used as the source for a
whole-disk flashing operation on a target with matching geometry and layout;
flashing an existing disk replaces its data. MCU flashing and hardware startup
are not validated by this host/browser workflow.
