#![no_std]
#![doc = include_str!("../README.md")]

mod error;
pub use error::{Error, Result};

#[cfg(feature = "sha256")]
mod hash;
#[cfg(feature = "sha256")]
pub use hash::{ContentVerifier, Sha256Verifier};

#[cfg(feature = "ed25519")]
mod signature;
#[cfg(feature = "ed25519")]
pub use signature::PublicKey;
