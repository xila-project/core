use std::io::{Error, ErrorKind};

use crate::Kind;

/// Map a standard library I/O error to the closest generic Xila kind.
///
/// Map raw operating system errors at the producer when they carry more
/// precise information than the standard library's error kind.
pub fn map_std_error(error: Error) -> Kind {
    match error.kind() {
        ErrorKind::PermissionDenied => Kind::PermissionDenied,
        ErrorKind::NotFound => Kind::NotFound,
        ErrorKind::AlreadyExists => Kind::AlreadyExists,
        ErrorKind::InvalidInput => Kind::InvalidArgument,
        ErrorKind::InvalidData => Kind::InvalidData,
        ErrorKind::TimedOut => Kind::TimedOut,
        ErrorKind::WouldBlock => Kind::WouldBlock,
        ErrorKind::Interrupted => Kind::Interrupted,
        ErrorKind::WriteZero => Kind::WriteZero,
        ErrorKind::OutOfMemory => Kind::OutOfMemory,
        ErrorKind::Unsupported => Kind::Unsupported,
        _ => Kind::InputOutput,
    }
}
