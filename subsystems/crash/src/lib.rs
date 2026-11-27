//! The XCR1 crash report format. Capture writes to caller-provided storage.

#![no_std]

mod boot_guard;
mod record;

pub use boot_guard::{BOOT_GUARD_SIZE, BootGuard, CRASH_LOOP_LIMIT};
pub use record::*;

#[cfg(feature = "persistence")]
mod persistence;
#[cfg(feature = "persistence")]
pub use persistence::*;

use core::sync::atomic::{Ordering, compiler_fence, fence};

fn checksum(bytes: &[u8]) -> u32 {
    let mut checksum = u32::MAX;
    for &byte in bytes {
        checksum ^= u32::from(byte);
        for _ in 0..8 {
            checksum = (checksum >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(checksum & 1));
        }
    }
    !checksum
}

fn write_marker(output: &mut [u8], marker: [u8; 4]) {
    compiler_fence(Ordering::SeqCst);
    fence(Ordering::SeqCst);
    for (byte, value) in output[..4].iter_mut().zip(marker) {
        // SAFETY: The byte is within the exclusive output slice.
        unsafe { core::ptr::write_volatile(byte, value) };
    }
    fence(Ordering::SeqCst);
    compiler_fence(Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    #[test]
    fn ieee_crc_32_check_value() {
        assert_eq!(super::checksum(b"123456789"), 0xCBF4_3926);
    }
}
