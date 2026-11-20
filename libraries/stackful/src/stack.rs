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
        let layout = Self::layout(size, min_usable).expect("stack too large");
        Self::allocate(layout).unwrap_or_else(|| handle_alloc_error(layout))
    }

    /// Like [`Stack::with_min_size`], but returns `None` instead of panicking or
    /// aborting when the size is unrepresentable or the allocator is out of memory.
    pub fn try_with_min_size(size: usize, min_usable: usize) -> Option<Self> {
        Self::allocate(Self::layout(size, min_usable)?)
    }

    fn layout(size: usize, min_usable: usize) -> Option<Layout> {
        let size = size.max(min_usable.max(MIN_STACK_SIZE));
        Layout::from_size_align(size.checked_add(GUARD + 15)? & !15, 16).ok()
    }

    fn allocate(layout: Layout) -> Option<Self> {
        let memory = NonNull::new(unsafe { alloc(layout) })?;
        unsafe { memory.as_ptr().write_bytes(PAINT, layout.size()) };
        Some(Self { memory, layout })
    }
    pub(crate) fn top(&self) -> usize {
        self.memory.as_ptr() as usize + self.layout.size()
    }
    pub fn usable_size(&self) -> usize {
        self.layout.size() - GUARD
    }
    pub fn guard_intact(&self) -> bool {
        // SAFETY: this Stack still owns the allocation containing the guard.
        unsafe { guard_intact_at(self.memory) }
    }
    /// The painted guard at the lower end, for checks while a coroutine owns the stack.
    pub(crate) fn guard(&self) -> NonNull<u8> {
        self.memory
    }
}

/// The corosensei view of a [`Stack`]. A private type, so only `Coroutine::new` can
/// hand a painted stack, which has no protected guard, to corosensei.
#[repr(transparent)]
pub(crate) struct Backing(pub(crate) Stack);

// SAFETY: the allocation is 16-byte aligned, stable for the lifetime of the value and
// at least 2 KiB. It has no guard page, which is acceptable here: Xila's targets have
// no MMU, so none is possible. The painted guard detects an overflow at the next switch,
// and the caller of the unsafe `Coroutine::new` sizes the stack to cover native code,
// its callees and interrupt frames (on Xtensa, `xtensa-lx-rt` pushes 256 bytes per
// interrupt level on the coroutine stack).
unsafe impl corosensei::stack::Stack for Backing {
    fn base(&self) -> corosensei::stack::StackPointer {
        // SAFETY: both addresses lie in a live allocation, so they are not zero. The
        // unchecked constructor avoids a panic site on every target.
        unsafe { corosensei::stack::StackPointer::new_unchecked(self.0.top()) }
    }
    fn limit(&self) -> corosensei::stack::StackPointer {
        // SAFETY: as in `base`.
        unsafe { corosensei::stack::StackPointer::new_unchecked(self.0.guard().as_ptr() as usize) }
    }
}

/// # Safety
/// The allocation holding the guard must be alive.
unsafe fn guard_intact_at(guard: NonNull<u8>) -> bool {
    (0..GUARD).all(|i| unsafe { guard.as_ptr().add(i).read_volatile() == PAINT })
}

/// Terminate the program if the painted guard was damaged.
///
/// # Safety
/// The allocation holding the guard must be alive.
pub(crate) unsafe fn check_guard_at(guard: NonNull<u8>) {
    // C ABI ensures fatal termination instead of unwinding over a damaged stack.
    extern "C" fn overflow() -> ! {
        panic!("stackful stack guard corrupted");
    }
    if !unsafe { guard_intact_at(guard) } {
        overflow();
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

    #[test]
    fn try_with_min_size_reports_failure_instead_of_aborting() {
        let stack = Stack::try_with_min_size(MIN_STACK_SIZE + 1, MIN_STACK_SIZE).unwrap();
        assert_eq!(stack.usable_size(), MIN_STACK_SIZE + 16);
        assert!(stack.guard_intact());
        // Unrepresentable and unobtainable sizes.
        assert!(Stack::try_with_min_size(usize::MAX, MIN_STACK_SIZE).is_none());
        assert!(Stack::try_with_min_size(isize::MAX as usize - 80, MIN_STACK_SIZE).is_none());
    }
}
