use core::fmt;

/// Verification failures never authorize activation or execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// SHA-256's maximum byte-aligned message length would be exceeded.
    MessageTooLong,
    /// Content is shorter or longer than its authenticated expected length.
    LengthMismatch,
    /// The calculated SHA-256 does not match the expected digest.
    DigestMismatch,
    /// An earlier update failed; the stream cannot be recovered.
    InvalidState,
    /// The public-key encoding is invalid or disallowed.
    InvalidPublicKey,
    /// An encoded key or signature has the wrong number of bytes.
    InvalidEncodingLength,
    /// Strict Ed25519 verification rejected the signature.
    InvalidSignature,
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MessageTooLong => "SHA-256 message length limit exceeded",
            Self::LengthMismatch => "content length mismatch",
            Self::DigestMismatch => "SHA-256 digest mismatch",
            Self::InvalidState => "verification stream previously failed",
            Self::InvalidPublicKey => "invalid Ed25519 public key",
            Self::InvalidEncodingLength => "invalid key or signature encoding length",
            Self::InvalidSignature => "invalid Ed25519 signature",
        })
    }
}

impl core::error::Error for Error {}
