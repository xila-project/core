use alloc::alloc::{alloc, dealloc, handle_alloc_error};
use core::{alloc::Layout, ptr::NonNull};
const GUARD: usize = 64;
/// Default minimum usable stack size in bytes applied by [`Stack::new`].
pub const DEFAULT_MIN_STACK_SIZE: usize = 4096;
/// Smallest usable stack size in bytes permitted by [`Stack::with_min_size`].
pub const MIN_STACK_SIZE: usize = 2048;
const PAINT: u8 = 0xa5;

/// Single heap allocation, aligned to 16 bytes, with a 64-byte painted lower guard.
/// This detects damage at boundaries; it does not provide memory protection.
pub struct Stack {
    memory: NonNull<u8>,
    layout: Layout,
}
impl Stack {
    /// Allocate a stack with at least [`DEFAULT_MIN_STACK_SIZE`] (4 KiB) usable bytes,
    /// rounded up to 16 bytes, plus a 64-byte painted lower guard.
    pub fn new(size: usize) -> Self {
        Self::with_min_size(size, DEFAULT_MIN_STACK_SIZE)
    }

    /// Allocate a stack with a caller-chosen minimum usable size, floored at
    /// [`MIN_STACK_SIZE`] (2 KiB), rounded up to 16 bytes, plus the same 64-byte
    /// painted lower guard as [`Stack::new`].
    ///
    /// Lowering the minimum below 4 KiB saves RAM on constrained MCUs (e.g. 256 KiB
    /// SRAM), at the cost of tighter headroom for call depth, debug frames,
    /// suspended futures pinned on the native stack, and shared interrupt frames.
    /// Audit worst-case stack usage before choosing a 2 KiB stack.
    pub fn with_min_size(size: usize, min_usable: usize) -> Self {
        let floor = min_usable.max(MIN_STACK_SIZE);
        let size = size
            .max(floor)
            .checked_add(GUARD + 15)
            .expect("stack too large")
            & !15;
        let layout = Layout::from_size_align(size, 16).expect("stack too large");
        let memory =
            NonNull::new(unsafe { alloc(layout) }).unwrap_or_else(|| handle_alloc_error(layout));
        unsafe { memory.as_ptr().write_bytes(PAINT, size) };
        Self { memory, layout }
    }
    pub(crate) fn top(&mut self) -> usize {
        self.memory.as_ptr() as usize + self.layout.size()
    }
    pub(crate) fn usable_limit(&self) -> usize {
        self.memory.as_ptr() as usize + GUARD
    }
    pub fn usable_size(&self) -> usize {
        self.layout.size() - GUARD
    }
    pub fn guard_intact(&self) -> bool {
        (0..GUARD).all(|i| unsafe { self.memory.as_ptr().add(i).read_volatile() == PAINT })
    }
    pub(crate) fn check_guard(&self) {
        // C ABI ensures fatal termination instead of unwinding over a damaged stack.
        extern "C" fn overflow() -> ! {
            panic!("stackful stack guard corrupted");
        }
        if !self.guard_intact() {
            overflow();
        }
    }
}
impl Drop for Stack {
    fn drop(&mut self) {
        unsafe { dealloc(self.memory.as_ptr(), self.layout) };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_guard_damage() {
        for stack in [
            Stack::new(DEFAULT_MIN_STACK_SIZE),
            Stack::with_min_size(MIN_STACK_SIZE, MIN_STACK_SIZE),
        ] {
            assert!(stack.guard_intact());
            unsafe { stack.memory.as_ptr().add(GUARD - 1).write(0) };
            assert!(!stack.guard_intact());
        }
    }

    #[test]
    fn respects_default_and_configurable_minimum_sizes() {
        assert_eq!(Stack::new(0).usable_size(), DEFAULT_MIN_STACK_SIZE);
        assert_eq!(
            Stack::new(MIN_STACK_SIZE).usable_size(),
            DEFAULT_MIN_STACK_SIZE
        );
        assert_eq!(
            Stack::with_min_size(MIN_STACK_SIZE, MIN_STACK_SIZE).usable_size(),
            MIN_STACK_SIZE
        );
        assert_eq!(Stack::with_min_size(512, 512).usable_size(), MIN_STACK_SIZE);
        assert_eq!(Stack::with_min_size(1024, 3072).usable_size(), 3072);
        assert_eq!(
            Stack::with_min_size(MIN_STACK_SIZE + 1, MIN_STACK_SIZE).usable_size(),
            MIN_STACK_SIZE + 16
        );
    }
}
