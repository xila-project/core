use crate::{Configuration, SampleFormat};
use core::{
    cell::UnsafeCell,
    marker::PhantomPinned,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use file_system::{Error, Result};
use nanoaudio::{Consumer, Mixer, Producer, RingBuffer, Volume};

const MIX_CONFIGURATION: Configuration = Configuration::new(48_000, 2, SampleFormat::F32);
const FRAME_BYTES: usize = 2 * size_of::<f32>();

// Contention is reported immediately; audio work never waits for another caller.
struct TryMutex<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: access to value requires the unique guard acquired with the atomic
// flag; Release on drop pairs with Acquire on the next successful attempt.
unsafe impl<T: Send> Sync for TryMutex<T> {}

impl<T> TryMutex<T> {
    fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    fn lock(&self) -> Result<Guard<'_, T>> {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| Error::RessourceBusy)?;
        Ok(Guard(self))
    }
}

struct Guard<'a, T>(&'a TryMutex<T>);
impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: this guard exclusively owns the lock until drop.
        unsafe { &*self.0.value.get() }
    }
}
impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: this guard exclusively owns the lock until drop.
        unsafe { &mut *self.0.value.get() }
    }
}
impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.0.locked.store(false, Ordering::Release);
    }
}

struct Stream<const N: usize> {
    bytes: [u8; N],
    read: usize,
    length: usize,
    started: bool,
}
impl<const N: usize> Stream<N> {
    fn new() -> Self {
        Self {
            bytes: [0; N],
            read: 0,
            length: 0,
            started: false,
        }
    }
}

struct OutputSlot<const N: usize> {
    ring: UnsafeCell<RingBuffer<N>>,
    dropped_blocks: AtomicUsize,
    occupied: AtomicBool,
}
// SAFETY: splitting is serialized by `streams` and only allowed after the previous
// consumer has been dropped. While occupied, only the SPSC halves touch the ring.
unsafe impl<const N: usize> Sync for OutputSlot<N> {}

/// A fixed pool of independent 48 kHz stereo interleaved F32 streams and outputs.
///
/// Defaults reserve 8 KiB for application PCM and 6 KiB for output PCM. The
/// fifth generic is bytes per sink (3072 = three 128-frame stereo F32 blocks).
/// Queue sizes must be frame-aligned; each output must hold a whole mix block.
/// Pin the system before registering outputs or mixing, using `core::pin::pin!`
/// for local storage or `alloc::boxed::Box::pin` for an owned platform service.
///
/// All storage is inline. Writes, controls, registration and mixing return
/// [`Error::RessourceBusy`] immediately on contention; no operation waits or spins.
/// Output consumption never takes the core lock.
///
/// ```compile_fail
/// use audio::AudioSystem;
/// let mut system: AudioSystem = AudioSystem::new().unwrap();
/// // A registered queue may not move, even when its handle is forgotten.
/// let pinned = core::pin::Pin::new(&mut system);
/// ```
pub struct AudioSystem<
    const STREAMS: usize = 4,
    const STREAM_BYTES: usize = 2048,
    const MIX_FRAMES: usize = 128,
    const OUTPUTS: usize = 2,
    const SINK_BYTES: usize = 3072,
> {
    _pinned: PhantomPinned,
    occupied: [AtomicBool; STREAMS],
    streams: TryMutex<Core<STREAMS, STREAM_BYTES, OUTPUTS, SINK_BYTES>>,
    outputs: [OutputSlot<SINK_BYTES>; OUTPUTS],
}

struct Core<const S: usize, const B: usize, const O: usize, const Q: usize> {
    streams: [Stream<B>; S],
    producers: [Option<Producer<'static, Q>>; O],
    mixer: Mixer<S>,
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>
    AudioSystem<S, B, F, O, Q>
{
    /// Creates empty, paused 48 kHz stereo F32 streams without allocation.
    /// Invalid const capacities return `InvalidParameter`.
    pub fn new() -> Result<Self> {
        let block_bytes = F.checked_mul(FRAME_BYTES).ok_or(Error::InvalidParameter)?;
        if B == 0
            || !B.is_multiple_of(FRAME_BYTES)
            || F == 0
            || Q < block_bytes
            || !Q.is_multiple_of(FRAME_BYTES)
        {
            return Err(Error::InvalidParameter);
        }
        Ok(Self {
            _pinned: PhantomPinned,
            occupied: core::array::from_fn(|_| AtomicBool::new(false)),
            streams: TryMutex::new(Core {
                streams: core::array::from_fn(|_| Stream::new()),
                producers: core::array::from_fn(|_| None),
                mixer: Mixer::new(),
            }),
            outputs: core::array::from_fn(|_| OutputSlot {
                ring: UnsafeCell::new(RingBuffer::new()),
                dropped_blocks: AtomicUsize::new(0),
                occupied: AtomicBool::new(false),
            }),
        })
    }

    /// Sets the master gain, from 0 to 100 percent.
    pub fn set_master_volume(&self, volume: u8) -> Result<()> {
        if volume > 100 {
            return Err(Error::InvalidParameter);
        }
        self.streams
            .lock()?
            .mixer
            .set_master_volume(Volume::from_percent(volume));
        Ok(())
    }

    /// Mixes one quantum and copies it to every output with room for a full block.
    /// A full output drops this block without affecting any other output.
    pub fn mix_next_block(self: Pin<&Self>) -> Result<()> {
        let mut core = self.streams.lock()?;
        let frame_size = FRAME_BYTES;
        let format = nanoaudio::Format::F32;
        let admitted: [bool; O] = core::array::from_fn(|index| {
            let occupied = self.outputs[index].occupied.load(Ordering::Acquire);
            if !occupied {
                return false;
            }
            let ready = core.producers[index]
                .as_ref()
                .is_some_and(|producer| producer.free() >= F * frame_size);
            if occupied && !ready {
                self.outputs[index]
                    .dropped_blocks
                    .fetch_add(1, Ordering::Relaxed);
            }
            occupied && ready
        });
        let active: [bool; S] =
            core::array::from_fn(|index| self.occupied[index].load(Ordering::Acquire));
        let Core {
            streams,
            producers,
            mixer,
        } = &mut *core;
        // One stereo F32 frame of scratch, independent of block length.
        // This avoids an additional persistent block beyond the queue budget.
        let mut frame = [0u8; FRAME_BYTES];
        for _ in 0..F {
            mixer.clear_bytes(&mut frame[..frame_size], format);
            for (index, stream) in streams.iter_mut().enumerate() {
                if !active[index] || !stream.started || stream.length == 0 {
                    continue;
                }
                mixer
                    .add_bytes(
                        index,
                        &mut frame[..frame_size],
                        &stream.bytes[stream.read..stream.read + frame_size],
                        format,
                    )
                    .map_err(|_| Error::InvalidParameter)?;
                stream.read = (stream.read + frame_size) % B;
                stream.length -= frame_size;
            }
            for (index, producer) in producers.iter_mut().enumerate() {
                if admitted[index] {
                    producer
                        .as_mut()
                        .expect("occupied output has producer")
                        .write(&frame[..frame_size]);
                }
            }
        }
        Ok(())
    }

    /// Reserves a sink and resets its queue and drop counter.
    pub fn open_output(self: Pin<&Self>) -> Result<OutputHandle<'_, S, B, F, O, Q>> {
        let this = self.get_ref();
        let mut core = self.streams.lock()?;
        let index = self
            .outputs
            .iter()
            .position(|slot| !slot.occupied.load(Ordering::Acquire))
            .ok_or(Error::RessourceBusy)?;
        // SAFETY: the core lock serializes registration and producer use. An
        // unoccupied slot has no live consumer; handle drop destroys it before
        // releasing the slot. Pinning prevents moves even if a handle is forgotten.
        core.producers[index].take();
        let ring = unsafe { &mut *this.outputs[index].ring.get() };
        let (producer, consumer) = ring.split();
        // SAFETY: this erased borrow is only used under the core lock while the
        // slot is occupied and this system is pinned. The ring stays at its
        // address until system drop, even if an output handle is forgotten.
        // On reuse the previous producer is dropped before splitting again.
        core.producers[index] = Some(unsafe {
            core::mem::transmute::<Producer<'_, Q>, Producer<'static, Q>>(producer)
        });
        self.outputs[index]
            .dropped_blocks
            .store(0, Ordering::Relaxed);
        self.outputs[index].occupied.store(true, Ordering::Release);
        Ok(OutputHandle {
            system: this,
            index,
            consumer: Some(consumer),
        })
    }

    /// Reserves an empty, paused application stream at 100 percent volume.
    pub fn open_stream(&self) -> Result<StreamHandle<'_, S, B, F, O, Q>> {
        let mut streams = self.streams.lock()?;
        let index = self
            .occupied
            .iter()
            .position(|slot| !slot.load(Ordering::Acquire))
            .ok_or(Error::RessourceBusy)?;
        streams.streams[index] = Stream::new();
        streams
            .mixer
            .set_volume(index, Volume::UNITY)
            .map_err(|_| Error::InvalidParameter)?;
        self.occupied[index].store(true, Ordering::Release);
        Ok(StreamHandle {
            system: self,
            index,
        })
    }
}

/// An exclusive reservation of one application stream; dropping it frees the slot.
pub struct StreamHandle<
    'a,
    const S: usize,
    const B: usize,
    const F: usize,
    const O: usize,
    const Q: usize,
> {
    system: &'a AudioSystem<S, B, F, O, Q>,
    index: usize,
}
impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>
    StreamHandle<'_, S, B, F, O, Q>
{
    pub fn configuration(&self) -> Configuration {
        MIX_CONFIGURATION
    }
    pub fn start(&self) -> Result<()> {
        self.system.streams.lock()?.streams[self.index].started = true;
        Ok(())
    }
    pub fn stop(&self) -> Result<()> {
        self.system.streams.lock()?.streams[self.index].started = false;
        Ok(())
    }
    pub fn flush(&self) -> Result<()> {
        if self.system.streams.lock()?.streams[self.index].length == 0 {
            Ok(())
        } else {
            Err(Error::RessourceBusy)
        }
    }
    pub fn set_volume(&self, volume: u8) -> Result<()> {
        if volume > 100 {
            return Err(Error::InvalidParameter);
        }
        self.system
            .streams
            .lock()?
            .mixer
            .set_volume(self.index, Volume::from_percent(volume))
            .map_err(|_| Error::InvalidParameter)
    }
    pub fn volume(&self) -> Result<u8> {
        Ok(self
            .system
            .streams
            .lock()?
            .mixer
            .volume(self.index)
            .map_err(|_| Error::InvalidParameter)?
            .percent() as u8)
    }
    /// Returns frame-aligned writable bytes, or zero while the core is busy.
    pub fn writable(&self) -> usize {
        self.system
            .streams
            .lock()
            .map(|streams| B - streams.streams[self.index].length)
            .unwrap_or(0)
    }
    /// Writes as many whole frames as fit. An incomplete input frame is rejected;
    /// the input byte slice does not need typed sample alignment.
    pub fn write(&self, bytes: &[u8]) -> Result<usize> {
        let frame_size = FRAME_BYTES;
        if !bytes.len().is_multiple_of(frame_size) {
            return Err(Error::InvalidParameter);
        }
        let mut streams = self.system.streams.lock()?;
        let stream = &mut streams.streams[self.index];
        let count = bytes.len().min(B - stream.length);
        let write = (stream.read + stream.length) % B;
        let first = count.min(B - write);
        stream.bytes[write..write + first].copy_from_slice(&bytes[..first]);
        stream.bytes[..count - first].copy_from_slice(&bytes[first..count]);
        stream.length += count;
        Ok(count)
    }
}
impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize> Drop
    for StreamHandle<'_, S, B, F, O, Q>
{
    fn drop(&mut self) {
        self.system.occupied[self.index].store(false, Ordering::Release);
    }
}

/// One independently drained output queue. Dropping it releases its output slot.
pub struct OutputHandle<
    'a,
    const S: usize,
    const B: usize,
    const F: usize,
    const O: usize,
    const Q: usize,
> {
    system: &'a AudioSystem<S, B, F, O, Q>,
    index: usize,
    consumer: Option<Consumer<'a, Q>>,
}
impl<'a, const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>
    OutputHandle<'a, S, B, F, O, Q>
{
    pub fn configuration(&self) -> Configuration {
        MIX_CONFIGURATION
    }
    /// Counts incoming whole blocks dropped while this sink had insufficient space.
    pub fn dropped_blocks(&self) -> usize {
        self.system.outputs[self.index]
            .dropped_blocks
            .load(Ordering::Relaxed)
    }
    /// Borrows the sole consumer without allowing it to escape the reservation.
    /// The callback executes without locking the audio core.
    pub fn with_consumer<R>(
        &mut self,
        consume: impl for<'c> FnOnce(&mut Consumer<'c, Q>) -> R,
    ) -> R {
        consume(
            self.consumer
                .as_mut()
                .expect("consumer exists until handle drop"),
        )
    }
}

/// Keeps a transferred native output slot reserved until the adapter is dropped.
///
/// This token is returned by [`OutputHandle::into_consumer_with_reservation`].
/// Drop the transferred consumer or its adapter before this reservation.
pub struct OutputReservation {
    occupied: &'static AtomicBool,
}

impl Drop for OutputReservation {
    fn drop(&mut self) {
        self.occupied.store(false, Ordering::Release);
    }
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize> Drop
    for OutputHandle<'_, S, B, F, O, Q>
{
    fn drop(&mut self) {
        if self.consumer.take().is_some() {
            self.system.outputs[self.index]
                .occupied
                .store(false, Ordering::Release);
        }
    }
}

impl<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>
    OutputHandle<'static, S, B, F, O, Q>
{
    /// Transfers a static system's consumer to an owning platform adapter.
    /// This permanently reserves the output slot: the adapter does not return
    /// its consumer, so the system cannot know when it is safe to reuse it.
    pub fn into_consumer(mut self) -> Consumer<'static, Q> {
        self.consumer
            .take()
            .expect("consumer exists until handle drop")
    }

    /// Transfers the consumer with a token that releases its output slot on drop.
    ///
    /// Unlike [`into_consumer`](Self::into_consumer), this permits the system to
    /// reuse the output after the consumer's owner has been dropped. The adapter
    /// must be destroyed before the returned reservation; a wrapper should own
    /// both and explicitly drop the adapter first.
    ///
    /// # Safety
    ///
    /// The returned consumer must be dropped before the reservation. Releasing
    /// the slot while the consumer is still live would allow the system to split
    /// the same ring buffer again while that consumer still references it.
    pub unsafe fn into_consumer_with_reservation(
        mut self,
    ) -> (Consumer<'static, Q>, OutputReservation) {
        let consumer = self
            .consumer
            .take()
            .expect("consumer exists until handle drop");
        let reservation = OutputReservation {
            occupied: &self.system.outputs[self.index].occupied,
        };
        (consumer, reservation)
    }
}
