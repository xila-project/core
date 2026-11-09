//! Audio character-device control contract (`device::kind::AUDIO`).
//!
//! Configuration is fixed for an open stream. [`GET_WRITABLE`] returns free
//! bytes. Volume is `0..=100` percent, applied when rendering. [`STOP`] pauses
//! on the next callback and retains queued PCM.

use file_system::{ControlCommand, Error, Result, define_command};

use crate::kind;

/// The little-endian representation of one PCM sample in the device byte stream.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// Unsigned 8-bit, with silence at 128.
    U8 = 0,
    I16 = 1,
    I24 = 2,
    I32 = 3,
    /// IEEE 754, with full scale at -1.0 and 1.0.
    F32 = 4,
}

impl SampleFormat {
    pub const fn sample_size(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16 => 2,
            Self::I24 => 3,
            Self::I32 | Self::F32 => 4,
        }
    }
}

impl TryFrom<u8> for SampleFormat {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::U8),
            1 => Ok(Self::I16),
            2 => Ok(Self::I24),
            3 => Ok(Self::I32),
            4 => Ok(Self::F32),
            _ => Err(Error::InvalidParameter),
        }
    }
}

/// Fixed-width device payload. Frames contain one sample per interleaved channel.
///
/// The format is stored as a byte so malformed command data cannot create an
/// invalid Rust enum. Reserved bytes must be zero; there is no implicit padding.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Configuration {
    pub sample_rate: u32,
    pub channels: u8,
    format: u8,
    reserved: [u8; 2],
}

impl Configuration {
    pub const fn new(sample_rate: u32, channels: u8, format: SampleFormat) -> Self {
        Self {
            sample_rate,
            channels,
            format: format as u8,
            reserved: [0; 2],
        }
    }

    pub fn sample_format(&self) -> Result<SampleFormat> {
        SampleFormat::try_from(self.format)
    }

    pub fn validate(&self) -> Result<()> {
        if self.sample_rate == 0 || !(1..=8).contains(&self.channels) || self.reserved != [0; 2] {
            return Err(Error::InvalidParameter);
        }
        self.sample_format()?;
        Ok(())
    }

    pub fn frame_size(&self) -> Result<usize> {
        self.validate()?;
        Ok(self.channels as usize * self.sample_format()?.sample_size())
    }
}

define_command!(
    SET_CONFIGURATION,
    Write,
    kind::AUDIO,
    0x01,
    Configuration,
    ()
);
define_command!(
    GET_CONFIGURATION,
    Read,
    kind::AUDIO,
    0x02,
    (),
    Configuration
);
define_command!(GET_WRITABLE, Read, kind::AUDIO, 0x03, (), u32);
define_command!(SET_VOLUME, Write, kind::AUDIO, 0x04, u8, ());
define_command!(GET_VOLUME, Read, kind::AUDIO, 0x05, (), u8);
define_command!(START, Write, kind::AUDIO, 0x06, (), ());
define_command!(STOP, Write, kind::AUDIO, 0x07, (), ());
