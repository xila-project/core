use crate::{checksum, write_marker};

pub const BOOT_GUARD_SIZE: usize = 16;
pub const CRASH_LOOP_LIMIT: u32 = 3;

const MAGIC: [u8; 4] = *b"XBG1";

/// Crash-loop counter in a separate retained slot. An invalid slot counts as zero.
pub struct BootGuard<'a>(&'a mut [u8; BOOT_GUARD_SIZE]);

impl<'a> BootGuard<'a> {
    pub fn new(storage: &'a mut [u8; BOOT_GUARD_SIZE]) -> Self {
        Self(storage)
    }

    pub fn consecutive_crashes(&self) -> u32 {
        if self.0[..4] != MAGIC || self.0[8..12] != checksum(&self.0[4..8]).to_le_bytes() {
            return 0;
        }
        u32::from_le_bytes(self.0[4..8].try_into().unwrap())
    }

    pub fn record_crash(&mut self) -> u32 {
        let count = self.consecutive_crashes().saturating_add(1);
        self.set_count(count);
        count
    }

    pub fn mark_stable(&mut self) {
        self.set_count(0);
    }

    fn set_count(&mut self, count: u32) {
        write_marker(self.0, [0; 4]);
        self.0[4..8].copy_from_slice(&count.to_le_bytes());
        let checksum = checksum(&self.0[4..8]);
        self.0[8..12].copy_from_slice(&checksum.to_le_bytes());
        self.0[12..].fill(0);
        write_marker(self.0, MAGIC);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_counter_until_stable_and_rejects_corruption() {
        let mut storage = [0; BOOT_GUARD_SIZE];
        for expected in 1..=CRASH_LOOP_LIMIT {
            assert_eq!(BootGuard::new(&mut storage).record_crash(), expected);
        }
        BootGuard::new(&mut storage).mark_stable();
        assert_eq!(BootGuard::new(&mut storage).consecutive_crashes(), 0);
        BootGuard::new(&mut storage).record_crash();
        storage[4] ^= 1;
        assert_eq!(BootGuard::new(&mut storage).consecutive_crashes(), 0);
    }
}
