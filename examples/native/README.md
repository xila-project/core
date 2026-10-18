# Native example

Prepare an independent reference disk, then run the example:

```sh
cargo make prepare-native-image
cargo run -p native_example -- --terminal --no-network \
  --seed-image target/seed/native/seed.img
```

Use `--gui` for the graphical interface. The default live disk is `./drive.img`;
select another with `--disk-path PATH`. The default seed path is `./seed.img`.
When packaging the executable, distribute a matching seed alongside it, or pass
its explicit path. The default disk size is 16 MiB; custom seeds and
`--disk-size BYTES` must agree.

Only an absent live disk is imported. Import validates a temporary copy before
publishing it without replacement. Existing disks do not require a seed and are
never resized or automatically reformatted, including empty or corrupt files.
A mount error requires inspecting or restoring that disk; to start another
installation, choose a different disk path. Do not delete a disk that contains
data you want to keep.

A new installation creates **administrator** with an empty password. Existing
accounts and passwords load on subsequent starts. Explicit pending installation
state permits completion after interruption without resetting saved credentials.
Applications and shortcuts are already in the image; startup performs no guest
compilation or bundled application installation. The native bundle includes
Calculator, Clock, Pixel Painter, Synth and Weather. Updating the reference image does
not replace applications on existing disks.

The `prebuilt-applications` feature remains accepted for older profiling commands,
but every build now uses the disk image. `XILA_PROFILE_APP_DIR` remains useful to
the profiling recorder for hashing source guest binaries; it no longer changes
runtime loading. See the repository's profiling instructions for preparing and
selecting a seed before measurement.

The example mounts `/devices/audio/playback` using the default host audio output.
Audio initialization failures are logged and leave the rest of the OS usable.
See [Synth](../../executables/synth/README.md) for controls, silent mode and checks.
