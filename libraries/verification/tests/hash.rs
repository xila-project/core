#![cfg(feature = "sha256")]

use verification::{ContentVerifier, Error, Sha256Verifier};

fn digest(hex: &str) -> [u8; 32] {
    core::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
}

// NIST SHA-256 examples / FIPS 180-4. Fixed independent expected values.
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn published_vectors_are_chunk_boundary_independent() {
    for (message, expected) in [
        (b"".as_slice(), EMPTY),
        (b"abc".as_slice(), ABC),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".as_slice(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ] {
        for size in [1, 7, 64, 512, 4096] {
            let mut hash = Sha256Verifier::new();
            let mut content = ContentVerifier::new(message.len() as u64, digest(expected)).unwrap();
            for chunk in message.chunks(size) {
                hash.update(chunk).unwrap();
                content.update(chunk).unwrap();
            }
            assert_eq!(hash.finalize().unwrap(), digest(expected));
            assert_eq!(content.verify(), Ok(()));
        }
    }
}

#[test]
fn million_a_vector_uses_constant_working_storage() {
    let mut hash = Sha256Verifier::new();
    for _ in 0..1000 {
        hash.update(&[b'a'; 1000]).unwrap();
    }
    assert_eq!(
        hash.finalize().unwrap(),
        digest("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0")
    );
}

#[test]
fn incomplete_and_modified_payloads_fail() {
    let mut short = ContentVerifier::new(3, digest(ABC)).unwrap();
    short.update(b"ab").unwrap();
    assert_eq!(short.verify(), Err(Error::LengthMismatch));
    let mut modified = ContentVerifier::new(3, digest(ABC)).unwrap();
    modified.update(b"abd").unwrap();
    assert_eq!(modified.verify(), Err(Error::DigestMismatch));
    assert_eq!(
        ContentVerifier::new(0, digest(ABC)).unwrap().verify(),
        Err(Error::DigestMismatch)
    );
}

#[test]
fn excess_bytes_poison_even_if_caller_ignores_error() {
    let mut content = ContentVerifier::new(3, digest(ABC)).unwrap();
    assert_eq!(content.update(b"abcd"), Err(Error::LengthMismatch));
    assert_eq!(content.update(b"abc"), Err(Error::InvalidState));
    assert_eq!(content.verify(), Err(Error::InvalidState));
    let mut empty = ContentVerifier::new(0, digest(EMPTY)).unwrap();
    assert_eq!(empty.update(b"x"), Err(Error::LengthMismatch));
    assert_eq!(empty.verify(), Err(Error::InvalidState));
}

#[test]
fn impossible_sha256_lengths_are_rejected_at_construction() {
    assert!(matches!(
        ContentVerifier::new(u64::MAX / 8 + 1, [0; 32]),
        Err(Error::MessageTooLong)
    ));
    assert!(ContentVerifier::new(u64::MAX / 8, [0; 32]).is_ok());
}
