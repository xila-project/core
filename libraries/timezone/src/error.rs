use core::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Unknown, case-mismatched or not compiled into this build. No UTC fallback.
    UnavailableZone,
    /// Outside the civil/timestamp range supported by the conversion engine.
    OutOfRange,
    /// UTC is unsynchronized or the core clock reported another failure.
    #[cfg(feature = "runtime-clock")]
    Clock(time::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnavailableZone => f.write_str("timezone is unavailable in this build"),
            Self::OutOfRange => f.write_str("timestamp is outside the supported civil time range"),
            #[cfg(feature = "runtime-clock")]
            Self::Clock(error) => error.fmt(f),
        }
    }
}

impl core::error::Error for Error {}
