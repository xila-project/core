//! Analog-to-digital converter contract.
//!
//! This contract is Xila-owned because embedded-hal 1.0 has no ADC trait. It
//! covers one-shot, blocking conversions only: no DMA and no continuous
//! sampling. A converter reports its geometry (channels, resolution and an
//! optional fixed reference voltage) and converts one channel per call.

#[cfg(feature = "mock")]
mod mock;

#[cfg(feature = "mock")]
pub use mock::AdcMock;

/// Error categories reported by a converter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The channel index is not implemented by the converter.
    InvalidChannel,
    /// The converter cannot start a conversion now.
    Busy,
    /// The converter does not provide the requested operation.
    Unsupported,
    /// Any other hardware or driver failure.
    Other,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidChannel => "invalid ADC channel",
            Self::Busy => "ADC busy",
            Self::Unsupported => "unsupported ADC operation",
            Self::Other => "ADC error",
        })
    }
}

impl core::error::Error for Error {}

/// Largest raw code for `resolution_bits`, saturating at 16 bits.
const fn full_scale(resolution_bits: u8) -> u16 {
    if resolution_bits >= 16 {
        u16::MAX
    } else {
        ((1u32 << resolution_bits) - 1) as u16
    }
}

/// Converts a raw sample to millivolts, where the full-scale code
/// `(1 << resolution_bits) - 1` maps to `reference_millivolts`. Codes above
/// full scale are clamped. Returns 0 when `resolution_bits` is 0.
pub const fn raw_to_millivolts(raw: u16, resolution_bits: u8, reference_millivolts: u32) -> u32 {
    let full_scale = full_scale(resolution_bits) as u64;
    if full_scale == 0 {
        return 0;
    }
    let code = if raw as u64 > full_scale {
        full_scale
    } else {
        raw as u64
    };
    (code * reference_millivolts as u64 / full_scale) as u32
}

/// Xila-owned contract for a one-shot ADC.
pub trait AdcController {
    /// Number of channels, indexed `0..channel_count()`.
    fn channel_count(&self) -> u8;

    /// Resolution of a raw sample, at most 16 bits. Samples are right-aligned.
    fn resolution_bits(&self) -> u8;

    /// Fixed reference voltage in millivolts, or `None` when the converter has none.
    fn reference_millivolts(&self) -> Option<u32>;

    /// Performs one blocking conversion of `channel`.
    fn read_raw(&mut self, channel: u8) -> Result<u16, Error>;
}

#[cfg(test)]
mod tests {
    use super::raw_to_millivolts;

    #[test]
    fn full_scale_maps_to_reference() {
        assert_eq!(raw_to_millivolts(4095, 12, 3300), 3300);
        assert_eq!(raw_to_millivolts(0, 12, 3300), 0);
        assert_eq!(raw_to_millivolts(2048, 12, 3300), 1650);
    }

    #[test]
    fn codes_above_full_scale_are_clamped() {
        assert_eq!(raw_to_millivolts(0xFFFF, 12, 3300), 3300);
        assert_eq!(raw_to_millivolts(0xFFFF, 16, 1000), 1000);
    }

    #[test]
    fn zero_resolution_yields_zero() {
        assert_eq!(raw_to_millivolts(1, 0, 3300), 0);
    }
}
