use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{Error, Result};

// FIPS 180-4 requires a message length strictly below 2^64 bits.
const MAX_BYTES: u64 = u64::MAX / 8;

/// Incremental SHA-256 with checked length and no heap allocation.
///
/// Any failed update permanently invalidates this state. Finalization consumes
/// it; dropping unfinished state does not report successful verification.
pub struct Sha256Verifier {
    hasher: Sha256,
    length: u64,
    failed: bool,
}

impl Default for Sha256Verifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256Verifier {
    pub fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            length: 0,
            failed: false,
        }
    }

    /// Hash borrowed logical bytes. Rejects messages longer than 2^61 - 1 bytes.
    pub fn update(&mut self, chunk: &[u8]) -> Result<()> {
        if self.failed {
            return Err(Error::InvalidState);
        }
        let next = u64::try_from(chunk.len())
            .ok()
            .and_then(|n| self.length.checked_add(n))
            .filter(|&n| n <= MAX_BYTES);
        let Some(next) = next else {
            self.failed = true;
            return Err(Error::MessageTooLong);
        };
        self.hasher.update(chunk);
        self.length = next;
        Ok(())
    }

    /// Produce a digest, not a proof of authenticity or installation success.
    pub fn finalize(self) -> Result<[u8; 32]> {
        if self.failed {
            return Err(Error::InvalidState);
        }
        Ok(self.hasher.finalize().into())
    }
}

/// Verify the exact logical length and SHA-256 declared by authenticated metadata.
///
/// Working memory is independent of content size. An I/O adapter must separately
/// abort on read/write/cancellation failures; a valid digest cannot repair them.
pub struct ContentVerifier {
    hash: Sha256Verifier,
    expected_length: u64,
    expected_digest: [u8; 32],
}

impl ContentVerifier {
    pub fn new(expected_length: u64, expected_digest: [u8; 32]) -> Result<Self> {
        if expected_length > MAX_BYTES {
            return Err(Error::MessageTooLong);
        }
        Ok(Self {
            hash: Sha256Verifier::new(),
            expected_length,
            expected_digest,
        })
    }

    /// Reject excess bytes before hashing and permanently poison failed streams.
    pub fn update(&mut self, chunk: &[u8]) -> Result<()> {
        if self.hash.failed {
            return Err(Error::InvalidState);
        }
        if u64::try_from(chunk.len())
            .ok()
            .and_then(|n| self.hash.length.checked_add(n))
            .is_none_or(|next| next > self.expected_length)
        {
            self.hash.failed = true;
            return Err(Error::LengthMismatch);
        }
        self.hash.update(chunk)
    }

    /// Consume the stream and check both length and digest. Partial streams fail.
    pub fn verify(self) -> Result<()> {
        if self.hash.failed {
            return Err(Error::InvalidState);
        }
        if self.hash.length != self.expected_length {
            return Err(Error::LengthMismatch);
        }
        if bool::from(self.hash.finalize()?.ct_eq(&self.expected_digest)) {
            Ok(())
        } else {
            Err(Error::DigestMismatch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_limit_failure_is_terminal() {
        let mut hash = Sha256Verifier::new();
        hash.length = MAX_BYTES - 1;
        assert_eq!(hash.update(b"a"), Ok(()));
        assert_eq!(hash.update(b"b"), Err(Error::MessageTooLong));
        assert_eq!(hash.update(b""), Err(Error::InvalidState));
        assert_eq!(hash.finalize(), Err(Error::InvalidState));
    }

    #[test]
    fn checked_counter_does_not_wrap() {
        let mut hash = Sha256Verifier::new();
        hash.length = u64::MAX;
        assert_eq!(hash.update(b"a"), Err(Error::MessageTooLong));
        assert_eq!(hash.finalize(), Err(Error::InvalidState));
    }
}
