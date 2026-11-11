//! Supported model file encodings for runtime loaders.
use file_system::{Error, Result};

/// Model serialization formats supported by inference backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ModelEncoding {
    /// TensorFlow Lite FlatBuffer format (`0`).
    TensorFlowLite = 0,
}

impl ModelEncoding {
    /// Convert a raw integer value into a [`ModelEncoding`], returning `None` if unknown.
    pub const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::TensorFlowLite),
            _ => None,
        }
    }

    /// Return the raw integer representation.
    pub const fn raw(self) -> u32 {
        self as u32
    }
}

impl TryFrom<u32> for ModelEncoding {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        Self::from_raw(value).ok_or(Error::InvalidParameter)
    }
}

impl From<ModelEncoding> for u32 {
    fn from(encoding: ModelEncoding) -> Self {
        encoding.raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_converts_to_and_from_raw() {
        assert_eq!(
            ModelEncoding::from_raw(0),
            Some(ModelEncoding::TensorFlowLite)
        );
        assert_eq!(ModelEncoding::from_raw(1), None);
        assert_eq!(
            ModelEncoding::try_from(0),
            Ok(ModelEncoding::TensorFlowLite)
        );
        assert_eq!(
            ModelEncoding::try_from(42),
            Err(file_system::Error::InvalidParameter)
        );
        assert_eq!(ModelEncoding::TensorFlowLite.raw(), 0);
        assert_eq!(u32::from(ModelEncoding::TensorFlowLite), 0);
    }
}
