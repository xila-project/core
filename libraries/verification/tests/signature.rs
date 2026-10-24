#![cfg(feature = "ed25519")]

use verification::{Error, PublicKey};

fn bytes<const N: usize>(hex: &str) -> [u8; N] {
    assert_eq!(hex.len(), N * 2);
    core::array::from_fn(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
}

// RFC 8032 section 7.1, TEST 1 and TEST 2 (public material only).
const KEY: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const SIG: &str = concat!(
    "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
    "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
);

#[test]
fn rfc8032_vectors_and_modified_messages() {
    for (key, signature, message) in [
        (KEY, SIG, b"".as_slice()),
        (
            "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
            concat!(
                "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da",
                "085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"
            ),
            b"\x72".as_slice(),
        ),
    ] {
        let key = PublicKey::from_bytes(&bytes(key)).unwrap();
        let signature = bytes(signature);
        assert_eq!(key.verify(message, &signature), Ok(()));
        assert_eq!(
            key.verify(b"modified", &signature),
            Err(Error::InvalidSignature)
        );
        for index in 0..64 {
            let mut changed = signature;
            changed[index] ^= 1;
            assert_eq!(key.verify(message, &changed), Err(Error::InvalidSignature));
        }
    }
}

#[test]
fn encodings_have_exact_lengths() {
    for length in [0, 31, 33] {
        assert!(matches!(
            PublicKey::from_slice(&[0; 33][..length]),
            Err(Error::InvalidEncodingLength)
        ));
    }
    let key = PublicKey::from_slice(&bytes::<32>(KEY)).unwrap();
    for length in [0, 63, 65] {
        assert_eq!(
            key.verify_slice(b"", &[0; 65][..length]),
            Err(Error::InvalidEncodingLength)
        );
    }
    assert_eq!(key.verify_slice(b"", &bytes::<64>(SIG)), Ok(()));
}

#[test]
fn weak_noncanonical_and_mixed_order_keys_are_rejected() {
    let mut identity = [0; 32];
    identity[0] = 1;
    let mut noncanonical_identity = [0xff; 32];
    noncanonical_identity[0] = 0xee; // p + 1
    noncanonical_identity[31] = 0x7f;
    let mut negative_zero_identity = identity;
    negative_zero_identity[31] = 0x80;
    for encoded in [
        [0; 32],
        identity,
        noncanonical_identity,
        negative_zero_identity,
    ] {
        assert!(matches!(
            PublicKey::from_bytes(&encoded),
            Err(Error::InvalidPublicKey)
        ));
    }
    let prime = ed25519_dalek::VerifyingKey::from_bytes(&bytes(KEY))
        .unwrap()
        .to_edwards();
    let torsion = ed25519_dalek::VerifyingKey::from_bytes(&[0; 32])
        .unwrap()
        .to_edwards();
    let mixed = (prime + torsion).compress().to_bytes();
    assert!(matches!(
        PublicKey::from_bytes(&mixed),
        Err(Error::InvalidPublicKey)
    ));
}

#[test]
fn wrong_key_small_order_r_and_malleable_scalar_fail() {
    let key = PublicKey::from_bytes(&bytes(KEY)).unwrap();
    let other_key = PublicKey::from_bytes(&bytes(
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
    ))
    .unwrap();
    assert_eq!(
        other_key.verify(b"", &bytes(SIG)),
        Err(Error::InvalidSignature)
    );
    let mut small_r = bytes::<64>(SIG);
    small_r[..32].fill(0);
    small_r[0] = 1;
    assert_eq!(key.verify(b"", &small_r), Err(Error::InvalidSignature));
    let mut scalar = bytes::<64>(SIG);
    // S + group order L, preserving the algebraic scalar but not its encoding.
    let order = bytes::<32>("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010");
    let mut carry = 0u16;
    for (s, l) in scalar[32..].iter_mut().zip(order) {
        let sum = u16::from(*s) + u16::from(l) + carry;
        *s = sum as u8;
        carry = sum >> 8;
    }
    assert_eq!(key.verify(b"", &scalar), Err(Error::InvalidSignature));
}
