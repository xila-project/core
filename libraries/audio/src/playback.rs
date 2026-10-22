use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice, Error,
    MountOperations, Result, Size,
};
use nanoaudio::{Consumer, Producer, RingBuffer, Sample, Stream, Volume};
use shared::AnyByLayout;
use synchronization::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

use crate::{
    Configuration, GET_CONFIGURATION, GET_VOLUME, GET_WRITABLE, SET_CONFIGURATION, SET_VOLUME,
    START, STOP,
};

fn validate_payload<T>(payload: &AnyByLayout) -> Result<()> {
    let pointer = core::ptr::from_ref(payload).cast::<u8>() as usize;
    if payload.get_size() != size_of::<T>() || !pointer.is_multiple_of(align_of::<T>()) {
        return Err(Error::InvalidParameter);
    }
    Ok(())
}

fn validate_command<C: ControlCommand>(input: &AnyByLayout, output: &AnyByLayout) -> Result<()> {
    validate_payload::<C::Input>(input)?;
    validate_payload::<C::Output>(output)
}

struct State {
    started: AtomicBool,
    failed: AtomicBool,
    volume: AtomicU8,
}

impl State {
    const fn new() -> Self {
        Self {
            started: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            volume: AtomicU8::new(100),
        }
    }

    fn check(&self) -> Result<()> {
        if self.failed.load(Ordering::Acquire) {
            Err(Error::InputOutput)
        } else {
            Ok(())
        }
    }
}

/// Caller-owned storage, including exactly `N` PCM bytes plus control/cursor state.
///
/// Allocate it once, or put it in static memory for an interrupt/host callback.
/// `N` must be nonzero and divisible by the configured frame size.
pub struct PlaybackBuffer<const N: usize> {
    ring: RingBuffer<N>,
    state: State,
}

impl<const N: usize> Default for PlaybackBuffer<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> PlaybackBuffer<N> {
    pub const fn new() -> Self {
        Self {
            ring: RingBuffer::new(),
            state: State::new(),
        }
    }

    /// Create a paused device and its single consumer, emptying previous data.
    /// The configuration is fixed until both halves have been dropped.
    pub fn split(
        &mut self,
        configuration: Configuration,
    ) -> Result<(OutputDevice<'_, N>, Renderer<'_, N>)> {
        let frame_size = configuration.frame_size()?;
        if N > u32::MAX as usize || !N.is_multiple_of(frame_size) {
            return Err(Error::InvalidParameter);
        }
        self.state = State::new();
        let (producer, consumer) = self.ring.split();
        Ok((
            OutputDevice {
                producer: Mutex::new(RefCell::new(producer)),
                configuration,
                frame_size,
                state: &self.state,
            },
            Renderer {
                consumer,
                configuration,
                frame_size,
                format: configuration.sample_format()?.native(),
                state: &self.state,
            },
        ))
    }
}

/// Serializes writers; the callback never takes this lock.
pub struct OutputDevice<'a, const N: usize> {
    producer: Mutex<CriticalSectionRawMutex, RefCell<Producer<'a, N>>>,
    configuration: Configuration,
    frame_size: usize,
    state: &'a State,
}

impl<const N: usize> DirectBaseOperations for OutputDevice<'_, N> {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }

    /// Enqueues whole frames, returning zero immediately when full. No alignment
    /// requirement on the byte slice; incomplete frames are rejected.
    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            self.state.check()?;
            if !buffer.len().is_multiple_of(self.frame_size) {
                return Err(Error::InvalidParameter);
            }
            Ok(self.producer.lock(|producer| {
                let mut producer = producer.borrow_mut();
                let free = producer.free() / self.frame_size * self.frame_size;
                producer.write(&buffer[..buffer.len().min(free)])
            }))
        })
    }

    /// Polls for queued PCM to be consumed. Success means submitted to the
    /// callback, not physically played by the hardware. Never waits or spins.
    fn flush(&self, _cx: &mut core::task::Context<'_>) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            self.state.check()?;
            if self.producer.lock(|producer| producer.borrow().free()) == N {
                Ok(())
            } else {
                Err(Error::RessourceBusy)
            }
        })
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<Result<()>> {
        file_system::ready(|| {
            self.state.check()?;
            match command {
                SET_CONFIGURATION::IDENTIFIER => {
                    validate_command::<SET_CONFIGURATION>(input, output)?;
                    let configuration = SET_CONFIGURATION::cast_input(input)?;
                    configuration.validate()?;
                    if *configuration != self.configuration {
                        return Err(Error::UnsupportedOperation);
                    }
                }
                GET_CONFIGURATION::IDENTIFIER => {
                    validate_command::<GET_CONFIGURATION>(input, output)?;
                    *GET_CONFIGURATION::cast_output(output)? = self.configuration;
                }
                GET_WRITABLE::IDENTIFIER => {
                    validate_command::<GET_WRITABLE>(input, output)?;
                    let free = self.producer.lock(|producer| producer.borrow().free());
                    *GET_WRITABLE::cast_output(output)? =
                        (free / self.frame_size * self.frame_size) as u32;
                }
                SET_VOLUME::IDENTIFIER => {
                    validate_command::<SET_VOLUME>(input, output)?;
                    let volume = *SET_VOLUME::cast_input(input)?;
                    if volume > 100 {
                        return Err(Error::InvalidParameter);
                    }
                    self.state.volume.store(volume, Ordering::Release);
                }
                GET_VOLUME::IDENTIFIER => {
                    validate_command::<GET_VOLUME>(input, output)?;
                    *GET_VOLUME::cast_output(output)? = self.state.volume.load(Ordering::Acquire);
                }
                START::IDENTIFIER => {
                    validate_command::<START>(input, output)?;
                    self.state.started.store(true, Ordering::Release);
                }
                STOP::IDENTIFIER => {
                    validate_command::<STOP>(input, output)?;
                    self.state.started.store(false, Ordering::Release);
                }
                _ => return Err(Error::UnsupportedOperation),
            }
            Ok(())
        })
    }
}

impl<const N: usize> MountOperations for OutputDevice<'_, N> {}
impl<const N: usize> DirectCharacterDevice for OutputDevice<'_, N> {}

/// A cheap handle for host error callbacks and backend teardown.
#[derive(Clone, Copy)]
pub struct FailureReporter<'a>(&'a AtomicBool);

impl FailureReporter<'_> {
    /// Marks this stream unusable. No allocation, locks or logging.
    pub fn fail(self) {
        self.0.store(true, Ordering::Release);
    }
}

/// The only consumer of a playback queue, owned by the hardware callback.
/// All render methods return frames consumed and fill the remainder with silence.
pub struct Renderer<'a, const N: usize> {
    consumer: Consumer<'a, N>,
    configuration: Configuration,
    frame_size: usize,
    format: nanoaudio::Format,
    state: &'a State,
}

impl<'a, const N: usize> Renderer<'a, N> {
    pub fn failure_reporter(&self) -> FailureReporter<'a> {
        FailureReporter(&self.state.failed)
    }

    pub fn render_f32(&mut self, output: &mut [f32]) -> usize {
        self.render(output)
    }

    pub fn render_i16(&mut self, output: &mut [i16]) -> usize {
        self.render(output)
    }

    pub fn render_i32(&mut self, output: &mut [i32]) -> usize {
        self.render(output)
    }

    pub fn render_u8(&mut self, output: &mut [u8]) -> usize {
        self.render(output)
    }

    fn render<S: Sample>(&mut self, output: &mut [S]) -> usize {
        output.fill(S::SILENCE);
        if self.state.check().is_err() || !self.state.started.load(Ordering::Acquire) {
            return 0;
        }
        let channels = self.configuration.channels as usize;
        let volume = Volume::from_percent(self.state.volume.load(Ordering::Acquire));
        let mut frames = 0;
        while output.len() - frames * channels >= channels {
            let bytes = self.consumer.readable();
            if bytes.is_empty() {
                break;
            }
            // Configuration and frame alignment were checked at split/write.
            let count = nanoaudio::source(bytes, self.format, self.configuration.channels)
                .expect("validated PCM configuration")
                .volume(volume)
                .write_samples(&mut output[frames * channels..]);
            self.consumer.release(count * self.frame_size);
            frames += count;
        }
        frames
    }
}

impl<const N: usize> Drop for Renderer<'_, N> {
    fn drop(&mut self) {
        self.failure_reporter().fail();
    }
}
