# Synth

A fixed-memory, eight-voice instrument running as a `wasm32-wasip1` guest in
Xila's Wasmi runtime, on both the native and browser examples. Build the example's
reference image (`cargo make prepare-native-image` or `prepare-wasm-image`) and
open **Synth** from the application drawer, or run:

```text
/applications/executables/wasm /applications/executables/synth
```

Hold a note to play; release it or move the pointer away to release the voice.
The key label gains `*` while held. Octave controls cover octaves 2 through 6;
changing octave stops held voices. **Sound** cycles sine, square and triangle
for newly triggered notes. Volume changes in steps of 10%, initially 70%.
The controls are session-local. The current input driver supplies a single
pointer; eight synthesis voices do not imply eight-finger touch support.

In the browser, click **Enable audio** on the page, then launch Synth. If it is
already open, click **Retry audio** inside Synth. This user gesture starts the
Web Audio context and mounts `/devices/audio/playback`. On native, the example
opens the default host audio output during boot. Linux builds require ALSA
development files (`libasound2-dev` on Debian/Ubuntu); the development shell and
CI install them. No audio device is required for boot or for the silent UI.

If opening or writing the stream fails, Synth remains usable in silent mode;
keys and controls still respond. Enable or repair the host output and use
**Retry audio**. Closing the app stops playback, closes its descriptor and
removes its window. At most one retained 128-frame PCM block is generated per
turn, with partial writes and backpressure handled without growing buffers.

New images include the binary and shortcut. An existing persistent disk is
preserved and does not automatically gain new apps: use the browser disk
selector to create a new disk, or a new `--disk-path` on native. Keep the previous
disk to retain its files and credentials. Package upgrades are tracked in #308.

## Validation

Host tests render nonzero PCM on note-on and silence after note-off, bound voices
and partial writes, and run the real guest through graphics events, WASI
`fd_control`, VFS playback and the mixer. These tests do not prove audible sound.
For an interactive smoke test on either example:

1. Launch Synth without audio available; hold/release a key and check its `*`.
2. Enable host audio, choose **Retry audio**, and check **Audio ready**.
3. Hold a key: check sound and visual feedback. Release or drag away: check silence.
4. Change octave, sound and volume; play again and check each change.
5. Close and reopen the application; check that playback stops and starts normally.

WT32-SC01 Plus speaker output still requires the I2S driver in #326 and a physical
board smoke test. Browser/native validation does not establish hardware support.
