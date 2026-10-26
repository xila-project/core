use core::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Unknown, case-mismatched or not compiled into this build. No UTC fallback.
    UnavailableZone,
    /// Outside the civil/timestamp range supported by the conversion engine.
    OutOfRange,
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnavailableZone => "timezone is unavailable in this build",
            Self::OutOfRange => "timestamp is outside the supported civil time range",
        })
    }
}

impl core::error::Error for Error {}
