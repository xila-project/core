//! Native resource probe; inputs and setup are outside timed sections.
use std::{hint::black_box, time::Instant};

fn main() {
    println!(
        "target={} profile=release chunks=512",
        std::env::consts::ARCH
    );
    #[cfg(feature = "sha256")]
    {
        use verification::{ContentVerifier, Sha256Verifier};
        println!(
            "SHA state={} content state={}",
            size_of::<Sha256Verifier>(),
            size_of::<ContentVerifier>()
        );
        let chunk = [0x5au8; 512];
        for size in [64 * 1024, 1024 * 1024, 4 * 1024 * 1024] {
            let mut samples = [0.0; 31];
            for sample in &mut samples {
                let started = Instant::now();
                let mut hash = Sha256Verifier::new();
                for _ in 0..size / chunk.len() {
                    hash.update(black_box(&chunk)).unwrap();
                }
                black_box(hash.finalize().unwrap());
                *sample = started.elapsed().as_secs_f64();
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "sha256 bytes={size} median_us={:.2} MiB/s={:.2}",
                samples[15] * 1e6,
                size as f64 / (1024.0 * 1024.0) / samples[15]
            );
        }
    }
    #[cfg(feature = "ed25519")]
    {
        use ed25519_dalek::{Signer, SigningKey};
        use verification::PublicKey;
        // Public test-only seed; never a deployment signing key. Fixture creation
        // is outside the timed section and only in this host example.
        let signer = SigningKey::from_bytes(&[0x42; 32]);
        let encoded = signer.verifying_key().to_bytes();
        let message = [0x5au8; 1024];
        let signature = signer.sign(&message).to_bytes();
        let key = PublicKey::from_bytes(&encoded).unwrap();
        println!("expanded key={}", size_of::<PublicKey>());
        for parse in [false, true] {
            let mut samples = [0.0; 301];
            for sample in &mut samples {
                let started = Instant::now();
                if parse {
                    black_box(PublicKey::from_bytes(black_box(&encoded)).unwrap());
                } else {
                    key.verify(black_box(&message), black_box(&signature))
                        .unwrap();
                }
                *sample = started.elapsed().as_secs_f64();
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{} median_us={:.2}",
                if parse {
                    "key-validation"
                } else {
                    "ed25519-1KiB"
                },
                samples[150] * 1e6
            );
        }
    }
    black_box(Instant::now());
}
