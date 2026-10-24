#![no_std]
#![no_main]

use core::hint::black_box;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "allocator-control")]
extern crate alloc;

/// Retained link/size probe, not runnable firmware or a package verifier.
///
/// # Safety
/// `message` must reference `length` readable bytes and `key`/`signature` must
/// reference initialized arrays, all immutable for the duration of this call.
/// `output` must reference 32 writable bytes not overlapping those inputs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn probe(
    message: *const u8,
    length: usize,
    key: *const [u8; 32],
    signature: *const [u8; 64],
    output: *mut [u8; 32],
) -> u32 {
    #[allow(unused_mut)]
    let mut result = 0u32;
    black_box((message, length, key, signature, output));
    #[cfg(any(feature = "sha256", feature = "ed25519"))]
    let message = unsafe { core::slice::from_raw_parts(message, length) };
    #[cfg(feature = "sha256")]
    {
        let mut hash = verification::Sha256Verifier::new();
        for chunk in message.chunks(512) {
            if hash.update(chunk).is_err() {
                return 1;
            }
        }
        match hash.finalize() {
            Ok(digest) => unsafe { output.write(digest) },
            Err(_) => return 1,
        }
    }
    #[cfg(feature = "ed25519")]
    {
        result |= match verification::PublicKey::from_bytes(unsafe { &*key }) {
            Ok(key) => {
                if key.verify(message, unsafe { &*signature }).is_ok() {
                    0
                } else {
                    2
                }
            }
            Err(_) => 3,
        };
    }
    #[cfg(feature = "allocator-control")]
    {
        black_box(alloc::boxed::Box::new(black_box(length)));
    }
    black_box(result)
}
