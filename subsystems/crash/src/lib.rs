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

pub(crate) fn crc32_step(checksum: u32, byte: u8) -> u32 {
    let mut checksum = checksum ^ u32::from(byte);
    for _ in 0..8 {
        checksum = (checksum >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(checksum & 1));
    }
    checksum
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut checksum = u32::MAX;
    for &byte in bytes {
        checksum = crc32_step(checksum, byte);
    }
    !checksum
}

fn write_marker(output: &mut [u8], marker: [u8; 4]) {
    // SAFETY: The slice is valid for writes for its lifetime.
    unsafe { write_marker_raw(output.as_mut_ptr(), marker) }
}

/// Write the 4-byte marker with volatile stores between fences, through a raw
/// pointer so that no Rust reference to the destination is created.
///
/// # Safety
/// `pointer..pointer.add(4)` must be valid for writes.
pub(crate) unsafe fn write_marker_raw(pointer: *mut u8, marker: [u8; 4]) {
    compiler_fence(Ordering::SeqCst);
    fence(Ordering::SeqCst);
    for (index, value) in marker.iter().enumerate() {
        // SAFETY: The caller guarantees the 4-byte range is writable.
        unsafe { core::ptr::write_volatile(pointer.add(index), *value) };
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
