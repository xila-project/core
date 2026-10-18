#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use nanoaudio::{Format, Stream};
use nanoaudio_synth::{Adsr, Patch, Synth, Waveform};

const fn control_identifier(direction: u32, payload_size: u32, number: u8) -> u32 {
    direction | (payload_size << 16) | ((b'A' as u32) << 8) | number as u32
}

pub const GET_CONFIGURATION: u32 = control_identifier(1 << 30, 8, 0x02);
pub const GET_WRITABLE: u32 = control_identifier(1 << 30, 4, 0x03);
pub const SET_VOLUME: u32 = control_identifier(2 << 30, 1, 0x04);
pub const GET_VOLUME: u32 = control_identifier(1 << 30, 1, 0x05);
pub const START: u32 = control_identifier(2 << 30, 0, 0x06);
pub const STOP: u32 = control_identifier(2 << 30, 0, 0x07);

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u8 = 2;
pub const BLOCK_FRAMES: usize = 128;
pub const VOICE_LIMIT: usize = 8;
pub const FRAME_BYTES: usize = CHANNELS as usize * size_of::<f32>();
pub const BLOCK_BYTES: usize = BLOCK_FRAMES * FRAME_BYTES;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioConfiguration {
    pub sample_rate: u32,
    pub channels: u8,
    pub format: u8,
    pub reserved: [u8; 2],
}

impl AudioConfiguration {
    pub const fn new(sample_rate: u32, channels: u8, format: u8) -> Self {
        Self {
            sample_rate,
            channels,
            format,
            reserved: [0; 2],
        }
    }
}

/// Held keys retain their MIDI pitch until release. Transposing clears held
/// keys; the caller must stop the instrument before changing octaves.
pub struct Keyboard {
    octave: u8,
    held: [Option<u8>; 13],
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            octave: 4,
            held: [None; 13],
        }
    }
}

impl Keyboard {
    pub fn octave(&self) -> u8 {
        self.octave
    }
    pub fn press(&mut self, key: usize) -> Option<u8> {
        let held = self.held.get_mut(key)?;
        if held.is_some() {
            return None;
        }
        let note = (self.octave + 1) * 12 + key as u8;
        *held = Some(note);
        Some(note)
    }
    pub fn release(&mut self, key: usize) -> Option<u8> {
        self.held.get_mut(key)?.take()
    }
    pub fn shift_octave(&mut self, delta: i8) -> bool {
        let octave = self.octave.saturating_add_signed(delta).clamp(2, 6);
        if octave == self.octave {
            return false;
        }
        self.octave = octave;
        self.held.fill(None);
        true
    }
}

/// Fixed-memory synthesizer and one retained PCM block for partial writes.
pub struct Instrument {
    synth: Synth<nanoaudio::DefaultRepresentation, VOICE_LIMIT>,
    patch: Patch,
    pcm: [u8; BLOCK_BYTES],
    pending: usize,
    offset: usize,
}

impl Instrument {
    pub fn new(configuration: AudioConfiguration) -> Result<Self, ConfigurationError> {
        if configuration.sample_rate == 0
            || !(1..=8).contains(&configuration.channels)
            || configuration.reserved != [0; 2]
        {
            return Err(ConfigurationError::Invalid);
        }
        if configuration.sample_rate != SAMPLE_RATE
            || configuration.channels != CHANNELS
            || configuration.format != 4
        {
            return Err(ConfigurationError::Unsupported);
        }

        Ok(Self {
            synth: Synth::new(SAMPLE_RATE),
            patch: Patch::new(
                Waveform::Sine,
                Adsr::from_millis(5, 20, 52_000, 90, SAMPLE_RATE),
            ),
            pcm: [0; BLOCK_BYTES],
            pending: 0,
            offset: 0,
        })
    }

    pub fn set_waveform(&mut self, waveform: Waveform) {
        self.patch.waveform = waveform;
    }

    pub fn note_on(&mut self, note: u8) {
        self.synth.note_on(0, note, 96, &self.patch);
    }

    pub fn note_off(&mut self, note: u8) {
        self.synth.note_off(0, note);
    }

    pub fn active_voices(&self) -> usize {
        self.synth.active_voices()
    }

    pub fn stop_all(&mut self) {
        self.synth.stop_all();
        self.pending = 0;
        self.offset = 0;
    }

    /// Writes at most one block. A short or zero write leaves the remaining
    /// bytes in the fixed buffer for the next call.
    pub fn write_once<W: PcmWriter>(
        &mut self,
        writable_bytes: usize,
        writer: &mut W,
    ) -> Result<WriteProgress, W::Error> {
        if self.pending == 0 {
            let bytes = writable_bytes.min(BLOCK_BYTES) / FRAME_BYTES * FRAME_BYTES;
            if bytes == 0 {
                return Ok(WriteProgress::Backpressure);
            }
            let mut stereo = (&mut self.synth)
                .to_channels(CHANNELS)
                .expect("stereo output is supported");
            let frames = stereo.write_bytes(&mut self.pcm[..bytes], Format::F32);
            self.pending = frames * FRAME_BYTES;
            self.offset = 0;
        }

        let available = writable_bytes.min(self.pending - self.offset) / FRAME_BYTES * FRAME_BYTES;
        if available == 0 {
            return Ok(WriteProgress::Backpressure);
        }
        let count = writer.write_pcm(&self.pcm[self.offset..self.offset + available])?;
        if count == 0 {
            return Ok(WriteProgress::Backpressure);
        }
        if count > available || !count.is_multiple_of(FRAME_BYTES) {
            return Err(writer.invalid_write());
        }
        self.offset += count;
        if self.offset == self.pending {
            self.pending = 0;
            self.offset = 0;
            Ok(WriteProgress::BlockSubmitted)
        } else {
            Ok(WriteProgress::Partial)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationError {
    Invalid,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteProgress {
    Partial,
    Backpressure,
    BlockSubmitted,
}

pub trait PcmWriter {
    type Error;

    fn write_pcm(&mut self, bytes: &[u8]) -> Result<usize, Self::Error>;
    fn invalid_write(&self) -> Self::Error;
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use audio::{
        GET_CONFIGURATION as DEVICE_GET_CONFIGURATION, GET_VOLUME as DEVICE_GET_VOLUME,
        GET_WRITABLE as DEVICE_GET_WRITABLE, SET_VOLUME as DEVICE_SET_VOLUME,
        START as DEVICE_START, STOP as DEVICE_STOP,
    };
    use file_system::ControlCommand;

    struct Sink {
        maximum: usize,
        bytes: Vec<u8>,
    }

    impl PcmWriter for Sink {
        type Error = &'static str;

        fn write_pcm(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
            let count = bytes.len().min(self.maximum);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }

        fn invalid_write(&self) -> Self::Error {
            "writer returned an invalid byte count"
        }
    }

    fn instrument() -> Instrument {
        Instrument::new(AudioConfiguration::new(SAMPLE_RATE, CHANNELS, 4)).unwrap()
    }

    #[test]
    fn keyboard_releases_pressed_pitch_and_ignores_duplicate_events() {
        let mut keys = Keyboard::default();
        assert_eq!(keys.press(0), Some(60));
        assert_eq!(keys.press(0), None);
        assert_eq!(keys.release(0), Some(60));
        assert_eq!(keys.release(0), None);
        assert_eq!(keys.press(12), Some(72));
        assert!(keys.shift_octave(1));
        assert_eq!(keys.release(12), None);
        assert_eq!(keys.press(0), Some(72));
        for _ in 0..10 {
            keys.shift_octave(1);
        }
        assert_eq!(keys.octave(), 6);
        for _ in 0..10 {
            keys.shift_octave(-1);
        }
        assert_eq!(keys.octave(), 2);
        assert_eq!(keys.press(99), None);
    }

    #[test]
    fn waveform_changes_the_rendered_audio() {
        let render = |waveform| {
            let mut instrument = instrument();
            instrument.set_waveform(waveform);
            instrument.note_on(60);
            let mut sink = Sink {
                maximum: BLOCK_BYTES,
                bytes: Vec::new(),
            };
            instrument.write_once(BLOCK_BYTES, &mut sink).unwrap();
            sink.bytes
        };
        assert_ne!(render(Waveform::Sine), render(Waveform::Square));
        assert_ne!(render(Waveform::Triangle), render(Waveform::Square));
    }

    #[test]
    fn note_on_generates_non_silent_pcm_and_note_off_releases_voice() {
        let mut instrument = instrument();
        instrument.note_on(60);

        let mut sink = Sink {
            maximum: BLOCK_BYTES,
            bytes: Vec::new(),
        };
        assert_eq!(
            instrument.write_once(BLOCK_BYTES, &mut sink),
            Ok(WriteProgress::BlockSubmitted)
        );
        assert!(
            sink.bytes
                .as_chunks::<4>()
                .0
                .iter()
                .any(|sample| *sample != [0; 4])
        );

        instrument.note_off(60);
        let mut release_sink = Sink {
            maximum: BLOCK_BYTES,
            bytes: Vec::new(),
        };
        for _ in 0..40 {
            let _ = instrument.write_once(BLOCK_BYTES, &mut release_sink);
        }
        assert_eq!(instrument.active_voices(), 0);
    }

    #[test]
    fn voice_allocation_stays_within_the_fixed_limit() {
        let mut instrument = instrument();
        for note in 60..=72 {
            instrument.note_on(note);
        }
        assert_eq!(instrument.active_voices(), VOICE_LIMIT);
    }

    #[test]
    fn short_and_zero_writes_retain_one_block_until_submitted() {
        let mut instrument = instrument();
        instrument.note_on(69);
        let mut sink = Sink {
            maximum: FRAME_BYTES * 17,
            bytes: Vec::new(),
        };

        assert_eq!(
            instrument.write_once(BLOCK_BYTES, &mut sink),
            Ok(WriteProgress::Partial)
        );
        let submitted_after_first = sink.bytes.len();
        sink.maximum = 0;
        assert_eq!(
            instrument.write_once(BLOCK_BYTES, &mut sink),
            Ok(WriteProgress::Backpressure)
        );
        assert_eq!(sink.bytes.len(), submitted_after_first);
        sink.maximum = FRAME_BYTES * 29;
        while sink.bytes.len() < BLOCK_BYTES {
            let _ = instrument.write_once(BLOCK_BYTES, &mut sink).unwrap();
        }
        assert_eq!(sink.bytes.len(), BLOCK_BYTES);
        assert_eq!(
            instrument.write_once(BLOCK_BYTES, &mut sink),
            Ok(WriteProgress::Partial)
        );
        assert_eq!(sink.bytes.len(), BLOCK_BYTES + FRAME_BYTES * 29);
    }

    #[test]
    fn rejects_formats_the_synth_cannot_render() {
        assert!(matches!(
            Instrument::new(AudioConfiguration::new(44_100, 2, 4)),
            Err(ConfigurationError::Unsupported)
        ));
        assert!(matches!(
            Instrument::new(AudioConfiguration::new(48_000, 2, 1)),
            Err(ConfigurationError::Unsupported)
        ));
    }

    #[test]
    fn generates_no_more_than_the_reported_writable_capacity() {
        let mut instrument = instrument();
        let mut sink = Sink {
            maximum: BLOCK_BYTES,
            bytes: Vec::new(),
        };
        assert_eq!(
            instrument.write_once(FRAME_BYTES * 3, &mut sink),
            Ok(WriteProgress::BlockSubmitted)
        );
        assert_eq!(sink.bytes.len(), FRAME_BYTES * 3);
        assert_eq!(
            instrument.write_once(0, &mut sink),
            Ok(WriteProgress::Backpressure)
        );
        assert_eq!(sink.bytes.len(), FRAME_BYTES * 3);
    }

    #[test]
    fn rejects_a_writer_count_larger_than_the_supplied_slice() {
        struct OverreportingWriter;

        impl PcmWriter for OverreportingWriter {
            type Error = &'static str;

            fn write_pcm(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
                Ok(bytes.len() + FRAME_BYTES)
            }

            fn invalid_write(&self) -> Self::Error {
                "writer returned an invalid byte count"
            }
        }

        let mut instrument = instrument();
        assert_eq!(
            instrument.write_once(FRAME_BYTES, &mut OverreportingWriter),
            Err("writer returned an invalid byte count")
        );
        assert_eq!(instrument.offset, 0);
    }

    #[test]
    fn generic_control_ids_match_the_audio_device_contract() {
        assert_eq!(
            GET_CONFIGURATION,
            DEVICE_GET_CONFIGURATION::IDENTIFIER.raw()
        );
        assert_eq!(GET_VOLUME, DEVICE_GET_VOLUME::IDENTIFIER.raw());
        assert_eq!(GET_WRITABLE, DEVICE_GET_WRITABLE::IDENTIFIER.raw());
        assert_eq!(SET_VOLUME, DEVICE_SET_VOLUME::IDENTIFIER.raw());
        assert_eq!(START, DEVICE_START::IDENTIFIER.raw());
        assert_eq!(STOP, DEVICE_STOP::IDENTIFIER.raw());
        assert_eq!(
            size_of::<AudioConfiguration>(),
            size_of::<audio::Configuration>()
        );
    }
}
