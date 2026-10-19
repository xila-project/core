#[derive(Debug)]
pub enum Error {
    InvalidPointer,
    InvalidLength,
    InvalidUtf8String,
    BufferTooSmall,
    FailedToConvertLengthToS,
}
