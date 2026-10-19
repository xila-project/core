use file_system::{ControlCommand, define_command};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashAlgorithm {
    Md5 = 0,
    Sha1 = 1,
    Sha224 = 2,
    Sha256 = 3,
    Sha384 = 4,
    Sha512 = 5,
    Sha512_224 = 6,
    Sha512_256 = 7,
}

define_command!(RESET, Write, b'H', 0, (), ());
define_command!(SET_ALGORITHM, Write, b'H', 1, HashAlgorithm, ());
