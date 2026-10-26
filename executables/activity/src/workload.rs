use alloc::vec::Vec;
use core::time::Duration;

/// A bounded workload owned and polled by the application's async loop.
/// No detached task, executor pool or background allocation survives Stop.
pub struct Workload {
    buffer: Vec<u8>,
    deadline: Duration,
    next_tick: Duration,
    offset: usize,
}

impl Workload {
    pub fn new(now: Duration) -> Result<Self, ()> {
        let mut buffer = Vec::new();
        buffer.try_reserve_exact(4096).map_err(|_| ())?;
        buffer.resize(4096, 0);
        Ok(Self {
            buffer,
            deadline: now + Duration::from_secs(10),
            next_tick: now,
            offset: 0,
        })
    }

    /// At most one 512-byte batch; missed batches are never replayed.
    /// Returns false when the owner should release this expired workload.
    pub fn tick(&mut self, now: Duration) -> bool {
        if now >= self.deadline {
            return false;
        }
        if now >= self.next_tick {
            for index in 0..512 {
                let byte = &mut self.buffer[(self.offset + index) % 4096];
                *byte = byte.wrapping_add(1);
            }
            core::hint::black_box(&self.buffer);
            self.offset = (self.offset + 512) % 4096;
            self.next_tick = now + Duration::from_millis(25);
        }
        true
    }

    pub fn next_wake(&self) -> Duration {
        self.next_tick.min(self.deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::time::Duration;

    #[test]
    fn demo_work_is_bounded_and_yields_between_batches() {
        let mut workload = Workload::new(Duration::ZERO).unwrap();
        assert!(workload.tick(Duration::ZERO));
        assert_eq!(
            workload.buffer.iter().filter(|byte| **byte == 1).count(),
            512
        );
        assert!(workload.tick(Duration::from_millis(24)));
        assert_eq!(
            workload.buffer.iter().filter(|byte| **byte == 1).count(),
            512
        );
        assert!(workload.tick(Duration::from_millis(25)));
        assert_eq!(
            workload.buffer.iter().filter(|byte| **byte == 1).count(),
            1024
        );
    }

    #[test]
    fn demo_expires_without_catching_up_missed_work() {
        let mut workload = Workload::new(Duration::from_secs(100)).unwrap();
        assert!(workload.tick(Duration::from_secs(105)));
        assert_eq!(
            workload.buffer.iter().filter(|byte| **byte == 1).count(),
            512
        );
        assert_eq!(workload.next_wake(), Duration::from_millis(105025));
        assert!(!workload.tick(Duration::from_secs(110)));
        assert_eq!(
            workload.buffer.iter().filter(|byte| **byte == 1).count(),
            512
        );
    }
}
