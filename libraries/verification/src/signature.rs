use ed25519_dalek::{Signature, VerifyingKey};

use crate::{Error, Result};

/// A validated Ed25519 public key, not a trust or authorization decision.
///
/// The caller must establish trust independently. Parsing rejects noncanonical,
/// small-order and mixed-order keys; verification uses Dalek's strict rules.
/// Store raw keys in Flash and validate on demand if retaining expanded keys
/// would exceed the target's persistent RAM budget.
pub struct PublicKey(VerifyingKey);

impl PublicKey {
    pub fn from_bytes(encoded: &[u8; 32]) -> Result<Self> {
        let key = VerifyingKey::from_bytes(encoded).map_err(|_| Error::InvalidPublicKey)?;
        let point = key.to_edwards();
        if point.compress().as_bytes() != encoded || key.is_weak() || !point.is_torsion_free() {
            return Err(Error::InvalidPublicKey);
        }
        Ok(Self(key))
    }

    /// Parse an untrusted encoding of exactly 32 bytes. This does not trust it.
    pub fn from_slice(encoded: &[u8]) -> Result<Self> {
        Self::from_bytes(
            encoded
                .try_into()
                .map_err(|_| Error::InvalidEncodingLength)?,
        )
    }

    /// Verify ordinary Ed25519 over the exact supplied bytes, not Ed25519ph.
    ///
    /// The caller defines and bounds the message (for example authenticated
    /// canonical metadata). A content digest is not interchangeable with the
    /// original message. No package schema or custom signing scheme is implied.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> Result<()> {
        self.0
            .verify_strict(message, &Signature::from_bytes(signature))
            .map_err(|_| Error::InvalidSignature)
    }

    /// Verify an encoded signature of exactly 64 bytes.
    pub fn verify_slice(&self, message: &[u8], signature: &[u8]) -> Result<()> {
        self.verify(
            message,
            signature
                .try_into()
                .map_err(|_| Error::InvalidEncodingLength)?,
        )
    }
}
