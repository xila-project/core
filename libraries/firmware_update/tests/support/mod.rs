use ed25519_dalek::{Signer, SigningKey};
use firmware_update::simulator::{MANIFEST_SIZE, TARGET_SIZE};
use verification::Sha256Verifier;
pub const TARGET: [u8; TARGET_SIZE] = *b"xila-native-test";
// Public deterministic test seed; never use for production firmware.
pub fn key() -> SigningKey {
    SigningKey::from_bytes(&[42; 32])
}
pub fn manifest(image: &[u8], version: u64) -> [u8; MANIFEST_SIZE] {
    let mut b = [0; MANIFEST_SIZE];
    b[..16].copy_from_slice(b"XILA-FW-SIM-v1\0\0");
    b[16..32].copy_from_slice(&TARGET);
    b[32..40].copy_from_slice(&(image.len() as u64).to_le_bytes());
    b[40..48].copy_from_slice(&version.to_le_bytes());
    let mut h = Sha256Verifier::new();
    h.update(image).unwrap();
    b[48..80].copy_from_slice(&h.finalize().unwrap());
    let sig = key().sign(&b[..80]).to_bytes();
    b[80..].copy_from_slice(&sig);
    b
}
