# PCM audio output

Initial audio support for [#124](https://github.com/xila-project/core/issues/124).
The `audio` crate defines a PCM character-device protocol and a bounded queue
with a callback renderer. It uses [nanoaudio](https://github.com/AlixANNERAUD/nanoaudio)
for buffering, conversion and volume, with only `buffer` and `dsp` enabled.
There are no codecs or resamplers in the OS audio dependency.

Enable the `audio` feature on `xila` to access `xila::audio`. Native playback is
separately enabled with `drivers_native/audio`; existing builds do not enable it.

## Device contract

Writes contain interleaved little-endian PCM. Supported input sample formats are
U8 (silence at 128), I16, packed I24, I32 and F32 (full scale -1 to 1).
The configuration specifies frames/second and 1 through 8 channels.
Its eight-byte `repr(C)` layout is `u32 sample_rate`, `u8 channels`, `u8 format`,
and two zero reserved bytes. Xila format values are U8=0, I16=1, I24=2, I32=3,
F32=4; they are independent of nanoaudio's discriminants.

Commands use the `b'A'` prefix:

| Number | Command | Input | Output |
| --- | --- | --- | --- |
| 1 | `SET_CONFIGURATION` | `Configuration` | `()` |
| 2 | `GET_CONFIGURATION` | `()` | `Configuration` |
| 3 | `GET_WRITABLE` | `()` | `u32` free bytes |
| 4 | `SET_VOLUME` | `u8`, 0..=100 percent | `()` |
| 5 | `GET_VOLUME` | `()` | `u8`, percent |
| 6 | `START` | `()` | `()` |
| 7 | `STOP` | `()` | `()` |

Command payloads must have exactly the declared size and alignment; malformed
payloads return `InvalidParameter` before reading or writing their contents.

- The configuration is fixed when splitting the storage/opening a backend.
  Setting it to the same value succeeds; a different valid value returns
  `UnsupportedOperation`. Open a new stream to change its configuration.
- New devices are paused, at 100% volume. You may prefill them before START.
  STOP pauses consumption and retains the queue; it takes effect on the next
  callback. An already executing callback can finish.
- `write()` never waits for playback: it accepts complete frames that fit and
  returns the number of bytes accepted, or zero when full. Input length must be
  a multiple of frame size; byte alignment is unrestricted. Retry unwritten
  bytes later, yielding between retries. Writers are serialized with a short
  critical section; the renderer never takes that lock.
- Underruns and paused callbacks produce silence. Volume applies to queued
  samples when they are consumed, without changing the input bytes.
- `flush()` is a **nonblocking drain poll**: `RessourceBusy` while queued and
  `Ok(())` when consumed by the callback. This does not guarantee that the last
  sample has left the host/device latency buffer. A paused queue needs START
  before it can drain.
- Capture/read is unsupported. Backend failure or dropping the renderer causes
  subsequent writes, controls and flush to return `InputOutput`.

## Storage and memory

Callers own a `PlaybackBuffer<N>`. `N` must be nonzero, fit in `u32`, and be
divisible by the input frame size. `split()` borrows that storage into a device
and exactly one renderer; dropping both permits reusing the storage.
Native callbacks require a static borrow so forgetting a runner cannot leave a
background thread borrowing a freed buffer. Static initialization or a one-time
allocation at application setup can supply it.

There are no allocations in queue setup or in Rust write/control/render/flush
processing. Rendering converts directly from contiguous ring spans to the
backend's output slice, without an intermediate PCM buffer. Calls are bounded
by the input/output size. The application must supply an appropriate platform
`critical-section` implementation; `drivers_native` enables its `std` version.

On the x86_64 debug test build, `PlaybackBuffer<4096>` occupies 4,120 bytes,
and the device and renderer each occupy 40 bytes. The test suite checks zero
allocations over 1,000 playback cycles after setup and concurrently transfers
5,000 samples across a wrapping ring. These figures exclude test buffers,
application stack and platform backend buffers/threads; they are not a
measurement of total host-driver memory or an assertion about backend
allocations.

## Fixed-capacity audio system

`AudioSystem<STREAMS, STREAM_BYTES, MIX_FRAMES, OUTPUTS, SINK_BYTES>` owns one
application queue per stream and one nanoaudio SPSC queue per output. The mix
contract is fixed at 48 kHz stereo interleaved F32; all stream and output handles
report that configuration. `new() -> Result<Self>` checks the const capacities.
Defaults are `AudioSystem<4, 2048, 128, 2, 3072>`:
four 256-frame application queues and two output queues holding three 128-frame
blocks each, for stereo F32. The fifth generic counts bytes so the type works on
stable Rust; sink storage must be frame-aligned and hold at least one mix block.

Pin the caller-owned system before opening outputs or mixing:

```rust
let system = core::pin::pin!(audio::AudioSystem::<4, 2048, 128, 2, 3072>::new()?);
let stream = system.open_stream()?;
let mut output = system.as_ref().open_output()?;
stream.start()?;
system.as_ref().mix_next_block()?;
output.with_consumer(|consumer| { /* platform adapter reads PCM here */ });
```

Opening beyond either limit returns `RessourceBusy`; dropping a handle releases
its slot. Reopened streams start empty and paused at 100 percent volume. Start,
stop, volume and drain polling follow the playback semantics above. Stream and
master gain use `nanoaudio::Mixer`; stopped or empty sources contribute silence.
Mixing processes each frame once and copies it to every admitted output. A sink
without room for the full block drops that incoming block, preserves its queued
PCM, and increments its own `dropped_blocks()` counter. Other sinks and application
queues continue. The next block can be admitted after its consumer drains space.

Writes, controls and mixing use an atomic try-lock: contention returns
`RessourceBusy` immediately; `writable()` conservatively returns zero while busy.
The scheduler should retry a skipped mix at its next opportunity. Output consumers
never take this lock. `with_consumer` keeps the consumer inside its reservation;
`OutputHandle<'static>::into_consumer` supports adapters that take ownership of a
static consumer, permanently reserving that output slot because those adapters do
not return their consumer for safe reuse. Adapters with an explicit owner can use
`into_consumer_with_reservation`, which returns the consumer and an
`OutputReservation` that reclaims the slot when dropped. This transfer is unsafe:
the adapter and consumer must be destroyed before the reservation. Keep both in
an owner that enforces this order, as the native `Runner` does.

### Headless output

`drivers_headless::HeadlessAudioOutput` reserves an output from the same pinned
`AudioSystem` used by physical adapters. It adds no queue, thread, timer or
allocation. Call `mix_next_block()` explicitly, then `read()` into a caller-owned
byte buffer to inspect interleaved stereo F32 PCM. Reads return available whole
frames without waiting; unused buffer bytes are unchanged. No samples are
invented for an empty queue. Mixing an empty or paused stream produces silence.
`dropped_blocks()` reports bounded-queue overflow, and dropping the driver frees
its output slot.

These tests run with normal workspace tests, without enabling
`drivers_native/audio` or opening an audio device. To test just the headless
crate, run `cargo test -p xila-drivers-headless`. Native-adapter checks remain opt-in
for work on the native example.

On the x86_64 debug test build, the system is **14,544 bytes**, each stream handle
is **16 bytes**, and each output handle is **24 bytes**. PCM queues contain exactly
**14,336 bytes (14 KiB)**: 8 KiB for application queues plus 6 KiB for output queues.
Mixing uses 8 bytes of stack PCM scratch (one stereo F32 frame), plus bounded
per-stream/per-output metadata; it does not store another whole mix block.
Defaults perform at most 1,024 source sample additions per 128-frame mix
(4 streams × 128 frames × 2 channels). Setup and steady-state core operations do
not allocate; a 1,000-cycle test checks all four writers, mixing, both output reads,
and volume controls with zero allocations. Platform buffers and adapter setup are
outside these measurements. No audible hardware/browser playback is claimed by
these host core tests.

## Local VFS playback file

`AudioStreamDevice` exposes the native/local service as a `CharacterDevice` for
the VFS node `/devices/audio/playback`. Construct it with a pinned static
`AudioSystem`, then register it at that path during system setup. Each VFS open
reserves one independent stream in that open's `Context`; writes and the
existing generic `b'A'` controls apply only to that stream. A close releases the
reservation, and reopening gets an empty paused stream at default volume.
Cloning an open context returns `UnsupportedOperation`, so stream handles are
never aliased. Reads are unsupported, and `flush()` polls until the mixer has
consumed queued PCM. Applications use ordinary file writes and descriptor
controls; the device does not add app-specific APIs or WASI imports.

## Native cpal output and example

The opt-in native backend uses `nanoaudio-cpal`, which selects host output
devices and adapts the fixed 48 kHz stereo F32 mix to their supported formats
and sample rates. `drivers_native::audio::new(system, output_count)` opens one
or two output sinks and starts one scheduler for the shared audio service. Each
cpal callback consumes its own bounded output queue, does not allocate or block,
and plays silence on underrun. Poll `Runner::check()` to detect mixer-thread
termination or host callback errors; `underruns()` reports underruns per sink.
Dropping the runner stops and joins the mixer before closing host streams.
It closes each cpal stream before releasing that sink's output reservation, so
the static audio system can register a replacement sink after runner teardown.
The same native driver targets (Linux, macOS and Windows) are supported; SDL
is no longer needed.

From the repository development shell:

```sh
cargo run -p xila-drivers-native --features audio --example audio
```

This plays a quiet two-second 440 Hz square-wave tone through the default host
output by writing to an ordinary audio stream. It does not require a GUI window.
The example allocates its pinned audio system during setup. An audible check
requires an available host output device; headless compilation does not test
playback.

```sh
cargo test -p xila-audio
```

## Later milestones

Web Audio, per-open VFS streams and the bounded generic guest `fd_control`
bridge are available. See [Synth](../../executables/synth/README.md) for the
native/browser example setup and smoke test. ESP32 I2S and system sounds remain
follow-up work. Applications can choose
nanoaudio codecs independently when they need decoding.
