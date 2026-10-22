pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, PartialEq, Clone, Copy, Eq)]
pub enum Error {
    Device(crate::Error),
    InvalidPartition,
    InvalidSignature,
    DeviceTooSmall,
    BufferTooSmall,
    NoValidPartitions,
    InvalidIndex,
    OverlappingPartitions,
    MultipleBootablePartitions,
    WriteFailed,
    ReadFailed,
    Full,
}

impl core::error::Error for Error {}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Device(e) => write!(f, "Device error: {}", e),
            Error::InvalidPartition => write!(f, "Invalid partition"),
            Error::InvalidSignature => write!(f, "Invalid MBR signature"),
            Error::DeviceTooSmall => write!(f, "Device is too small for MBR"),
            Error::BufferTooSmall => write!(f, "Provided buffer is too small"),
            Error::NoValidPartitions => write!(f, "No valid partitions found"),
            Error::InvalidIndex => write!(f, "Invalid partition index"),
            Error::OverlappingPartitions => write!(f, "Partitions are overlapping"),
            Error::MultipleBootablePartitions => write!(f, "Multiple bootable partitions found"),
            Error::WriteFailed => write!(f, "Failed to write data"),
            Error::ReadFailed => write!(f, "Failed to read data"),
            Error::Full => write!(f, "No free partition slots available"),
        }
    }
}

impl From<crate::Error> for Error {
    fn from(value: crate::Error) -> Self {
        Error::Device(value)
    }
}
