//! Native output for the fixed-capacity Xila audio service.

use ::audio::AudioSystem;
use ::audio::OutputReservation;
use nanoaudio_cpal::{Output, cpal::traits::HostTrait};
use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u8 = 2;
const MAX_OUTPUTS: usize = 2;

fn mix_period(frames: usize) -> Duration {
    let nanos = frames as u128 * 1_000_000_000 / SAMPLE_RATE as u128;
    Duration::from_nanos(nanos.min(u64::MAX as u128) as u64)
}

fn output_configuration(
    configuration: ::audio::Configuration,
) -> Result<nanoaudio::Configuration, Error> {
    let format = match configuration.sample_format().map_err(Error::System)? {
        ::audio::SampleFormat::U8 => nanoaudio::Format::U8,
        ::audio::SampleFormat::I16 => nanoaudio::Format::I16,
        ::audio::SampleFormat::I24 => nanoaudio::Format::I24,
        ::audio::SampleFormat::I32 => nanoaudio::Format::I32,
        ::audio::SampleFormat::F32 => nanoaudio::Format::F32,
    };
    Ok(nanoaudio::Configuration::new(
        configuration.sample_rate,
        configuration.channels,
        format,
    ))
}

#[derive(Debug)]
pub enum Error {
    InvalidOutputCount,
    System(file_system::Error),
    Open(nanoaudio_cpal::Error),
    Play(nanoaudio_cpal::Error),
    MixFailed,
    MixThread(std::io::Error),
    OutputFailed,
    MixerStopped,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOutputCount => write!(
                formatter,
                "native audio output count must be 1..={MAX_OUTPUTS}"
            ),
            Self::System(error) => {
                write!(formatter, "failed to reserve native audio output: {error}")
            }
            Self::Open(error) => write!(formatter, "failed to open native audio output: {error}"),
            Self::Play(error) => write!(formatter, "failed to start native audio output: {error}"),
            Self::MixFailed => write!(formatter, "native audio mixer failed"),
            Self::MixThread(error) => {
                write!(formatter, "failed to start native audio mixer: {error}")
            }
            Self::OutputFailed => write!(formatter, "native audio output reported a runtime error"),
            Self::MixerStopped => write!(formatter, "native audio mixer stopped unexpectedly"),
        }
    }
}

impl std::error::Error for Error {}

/// Owns the native output streams and the one scheduler that mixes for all sinks.
///
/// Output streams are opened during setup. The mixer thread only renders fixed
/// 128-frame quanta; cpal callbacks consume independent queues and insert
/// silence on underrun. Drop stops and joins the mixer before closing outputs.
pub struct Runner {
    outputs: Vec<NativeSink<Output, OutputReservation>>,
    stop: Arc<AtomicBool>,
    mixer_failed: Arc<AtomicBool>,
    mixer: Option<JoinHandle<()>>,
}

struct NativeSink<T, L> {
    output: Option<T>,
    reservation: Option<L>,
}

impl<T, L> NativeSink<T, L> {
    fn new(output: T, reservation: L) -> Self {
        Self {
            output: Some(output),
            reservation: Some(reservation),
        }
    }

    fn output(&self) -> &T {
        self.output.as_ref().expect("output exists until sink drop")
    }
}

impl<T, L> Drop for NativeSink<T, L> {
    fn drop(&mut self) {
        drop(self.output.take());
        drop(self.reservation.take());
    }
}

fn build_native_sink<const Q: usize, T, E>(
    consumer: nanoaudio::Consumer<'static, Q>,
    reservation: OutputReservation,
    build: impl FnOnce(nanoaudio::Consumer<'static, Q>) -> Result<T, E>,
) -> Result<NativeSink<T, OutputReservation>, E> {
    let output = build(consumer)?;
    Ok(NativeSink::new(output, reservation))
}

impl Runner {
    /// Returns output underruns accumulated by each native sink.
    pub fn underruns(&self) -> impl Iterator<Item = u32> + '_ {
        self.outputs.iter().map(|sink| sink.output().underruns())
    }

    /// Returns output callback errors accumulated by each native sink.
    pub fn output_errors(&self) -> impl Iterator<Item = u32> + '_ {
        self.outputs.iter().map(|sink| sink.output().errors())
    }

    /// Polls host output and mixer health without blocking.
    pub fn check(&self) -> Result<(), Error> {
        if self.mixer_failed.load(Ordering::Acquire) {
            return Err(Error::MixFailed);
        }
        if self.mixer.as_ref().is_some_and(JoinHandle::is_finished) {
            return Err(Error::MixerStopped);
        }
        if self.outputs.iter().any(|sink| sink.output().errors() != 0) {
            return Err(Error::OutputFailed);
        }
        Ok(())
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(mixer) = self.mixer.take() {
            let _ = mixer.join();
        }
    }
}

/// Opens `output_count` host devices and starts one mixer for their shared mix.
///
/// The audio system must live in pinned static storage because cpal's callback
/// owns its output consumer for the lifetime of each stream. Only setup allocates;
/// the scheduling, mixing and cpal rendering paths use bounded queues. cpal
/// adapts the fixed 48 kHz stereo F32 mix to each selected device configuration.
pub fn new<const S: usize, const B: usize, const F: usize, const O: usize, const Q: usize>(
    system: Pin<&'static AudioSystem<S, B, F, O, Q>>,
    output_count: usize,
) -> Result<Runner, Error> {
    if !(1..=MAX_OUTPUTS).contains(&output_count) || output_count > O {
        return Err(Error::InvalidOutputCount);
    }
    let host = nanoaudio_cpal::cpal::default_host();
    let mut devices = host
        .output_devices()
        .map_err(|_| Error::Open(nanoaudio_cpal::Error::NoDevice))?;
    let mut outputs = Vec::with_capacity(output_count);
    for _ in 0..output_count {
        let device = devices
            .next()
            .ok_or_else(|| Error::Open(nanoaudio_cpal::Error::NoDevice))?;
        let sink = system.as_ref().open_output().map_err(Error::System)?;
        let configuration = output_configuration(sink.configuration())?;
        let (consumer, reservation) = unsafe { sink.into_consumer_with_reservation() };
        let sink = build_native_sink(consumer, reservation, |consumer| {
            Output::open_device(
                &device,
                consumer,
                configuration,
                nanoaudio_cpal::Quality::default(),
            )
            .map_err(Error::Open)
        })?;
        sink.output().play().map_err(Error::Play)?;
        outputs.push(sink);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let mixer_failed = Arc::new(AtomicBool::new(false));
    let thread_failed = Arc::clone(&mixer_failed);
    let mixer = thread::Builder::new()
        .name("xila-audio-mixer".into())
        .spawn(move || {
            let period = mix_period(F);
            let mut deadline = Instant::now();
            while !thread_stop.load(Ordering::Acquire) {
                if let Err(error) = system.as_ref().mix_next_block() {
                    if error != file_system::Error::RessourceBusy {
                        thread_failed.store(true, Ordering::Release);
                        return;
                    }
                }
                deadline += period;
                let now = Instant::now();
                if deadline > now {
                    thread::sleep(deadline - now);
                } else {
                    // Do not run catch-up bursts after a delayed host schedule.
                    deadline = now;
                }
            }
        })
        .map_err(Error::MixThread)?;
    Ok(Runner {
        outputs,
        stop,
        mixer_failed,
        mixer: Some(mixer),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_fixed_system_mix_configuration() {
        let configuration = output_configuration(::audio::Configuration::new(
            SAMPLE_RATE,
            CHANNELS,
            ::audio::SampleFormat::F32,
        ))
        .unwrap();
        assert_eq!(configuration.sample_rate, 48_000);
        assert_eq!(configuration.channels, 2);
        assert_eq!(configuration.format, nanoaudio::Format::F32);
    }

    #[test]
    fn schedules_one_block_per_mix_period() {
        assert_eq!(mix_period(128).as_nanos(), 2_666_666);
        assert_eq!(mix_period(64).as_nanos(), 1_333_333);
        assert_eq!(mix_period(0), Duration::ZERO);
    }

    #[test]
    fn rejects_output_counts_outside_the_fixed_sink_limit() {
        assert_eq!(MAX_OUTPUTS, 2);
        assert!(!(1..=MAX_OUTPUTS).contains(&0));
        assert!(!(1..=MAX_OUTPUTS).contains(&(MAX_OUTPUTS + 1)));
    }

    #[test]
    fn invalid_output_count_fails_before_accessing_host_devices() {
        let system: Pin<&'static AudioSystem> =
            Box::leak(Box::pin(AudioSystem::new().unwrap())).as_ref();
        assert!(matches!(new(system, 0), Err(Error::InvalidOutputCount)));
        assert!(matches!(new(system, 3), Err(Error::InvalidOutputCount)));
    }

    #[test]
    fn output_capacity_is_checked_before_host_device_access() {
        type OneOutputSystem = AudioSystem<4, 2048, 128, 1, 3072>;
        let system: Pin<&'static OneOutputSystem> =
            Box::leak(Box::pin(OneOutputSystem::new().unwrap())).as_ref();
        assert!(matches!(new(system, 2), Err(Error::InvalidOutputCount)));
        assert!(system.as_ref().open_output().is_ok());
    }

    #[test]
    fn native_sink_drops_backend_before_releasing_its_reservation() {
        use std::{cell::RefCell, rc::Rc};

        struct DropProbe(&'static str, Rc<RefCell<Vec<&'static str>>>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.1.borrow_mut().push(self.0);
            }
        }

        let dropped = Rc::new(RefCell::new(Vec::new()));
        drop(NativeSink::new(
            DropProbe("backend", Rc::clone(&dropped)),
            DropProbe("reservation", Rc::clone(&dropped)),
        ));
        assert_eq!(*dropped.borrow(), ["backend", "reservation"]);
    }

    #[test]
    fn dropping_native_sink_reclaims_its_system_slot() {
        let system: Pin<&'static AudioSystem> =
            Box::leak(Box::pin(AudioSystem::new().unwrap())).as_ref();
        let (consumer, reservation) = unsafe {
            system
                .as_ref()
                .open_output()
                .unwrap()
                .into_consumer_with_reservation()
        };
        let sink = NativeSink::new(consumer, reservation);
        let second = system.as_ref().open_output().unwrap();
        assert!(matches!(
            system.as_ref().open_output(),
            Err(file_system::Error::RessourceBusy)
        ));
        drop(sink);
        let third = system.as_ref().open_output().unwrap();
        drop((second, third));
    }

    #[test]
    fn failed_backend_open_releases_its_reservation_after_consumer_drop() {
        let system: Pin<&'static AudioSystem> =
            Box::leak(Box::pin(AudioSystem::new().unwrap())).as_ref();
        let (consumer, reservation) = unsafe {
            system
                .as_ref()
                .open_output()
                .unwrap()
                .into_consumer_with_reservation()
        };
        let second = system.as_ref().open_output().unwrap();
        let error = build_native_sink(consumer, reservation, |consumer| {
            drop(consumer);
            Err::<(), _>("adapter setup failed")
        });
        assert!(matches!(error, Err("adapter setup failed")));
        let third = system.as_ref().open_output().unwrap();
        drop((second, third));
    }
}
