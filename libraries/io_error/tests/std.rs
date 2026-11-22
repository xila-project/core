#![cfg(feature = "std")]

use io_error::{Kind, map_std_error};
use std::io::{Error, ErrorKind};

#[test]
fn preserves_the_generic_host_mapping() {
    for (host, expected) in [
        (ErrorKind::PermissionDenied, Kind::PermissionDenied),
        (ErrorKind::NotFound, Kind::NotFound),
        (ErrorKind::AlreadyExists, Kind::AlreadyExists),
        (ErrorKind::InvalidInput, Kind::InvalidArgument),
        (ErrorKind::InvalidData, Kind::InvalidData),
        (ErrorKind::TimedOut, Kind::TimedOut),
        (ErrorKind::WouldBlock, Kind::WouldBlock),
        (ErrorKind::Interrupted, Kind::Interrupted),
        (ErrorKind::WriteZero, Kind::WriteZero),
        (ErrorKind::OutOfMemory, Kind::OutOfMemory),
        (ErrorKind::Unsupported, Kind::Unsupported),
        (ErrorKind::Other, Kind::InputOutput),
        (ErrorKind::UnexpectedEof, Kind::InputOutput),
    ] {
        assert_eq!(map_std_error(Error::from(host)), expected, "{host:?}");
    }
}
