use alloc::alloc::{alloc, dealloc, handle_alloc_error};
use core::{alloc::Layout, ptr::NonNull};
const GUARD: usize = 64;
const PAINT: u8 = 0xa5;

/// Single heap allocation, aligned to 16 bytes, with a 64-byte painted lower guard.
/// This detects damage at boundaries; it does not provide memory protection.
pub struct Stack {
    memory: NonNull<u8>,
    layout: Layout,
}
impl Stack {
    pub fn new(size: usize) -> Self {
        let size = size
            .max(4096)
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
        let stack = Stack::new(4096);
        assert!(stack.guard_intact());
        unsafe { stack.memory.as_ptr().add(GUARD - 1).write(0) };
        assert!(!stack.guard_intact());
    }
}
