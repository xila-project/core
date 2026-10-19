use file_system::{Error, Result};

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

    pub(crate) const fn native(self) -> nanoaudio::Format {
        match self {
            Self::U8 => nanoaudio::Format::U8,
            Self::I16 => nanoaudio::Format::I16,
            Self::I24 => nanoaudio::Format::I24,
            Self::I32 => nanoaudio::Format::I32,
            Self::F32 => nanoaudio::Format::F32,
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
