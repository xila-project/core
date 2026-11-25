use file_system::{ControlCommand, IoErrorKind, Result, define_command};

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

impl TryFrom<u8> for HashAlgorithm {
    type Error = IoErrorKind;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Md5),
            1 => Ok(Self::Sha1),
            2 => Ok(Self::Sha224),
            3 => Ok(Self::Sha256),
            4 => Ok(Self::Sha384),
            5 => Ok(Self::Sha512),
            6 => Ok(Self::Sha512_224),
            7 => Ok(Self::Sha512_256),
            _ => Err(IoErrorKind::InvalidArgument),
        }
    }
}

define_command!(RESET, Write, crate::kind::HASH, 0, (), ());
// The input is the algorithm as a byte, not the enum itself: a payload byte
// pattern such as `200` must not be read as an invalid `HashAlgorithm`.
// Readers convert with `TryFrom` and reject unknown values with
// `InvalidArgument`.
define_command!(SET_ALGORITHM, Write, crate::kind::HASH, 1, u8, ());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_algorithm_try_from_rejects_unknown_bytes() {
        assert_eq!(
            HashAlgorithm::try_from(HashAlgorithm::Sha256 as u8).unwrap(),
            HashAlgorithm::Sha256
        );
        assert_eq!(
            HashAlgorithm::try_from(200),
            Err(IoErrorKind::InvalidArgument)
        );
    }
}
