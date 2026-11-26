use alloc::alloc::{alloc, dealloc, handle_alloc_error};
use core::{alloc::Layout, ptr::NonNull};
const GUARD: usize = 64;
/// Default minimum usable stack size in bytes applied by [`Stack::new`].
pub const DEFAULT_MIN_STACK_SIZE: usize = 4096;
/// Smallest usable stack size in bytes permitted by [`Stack::with_min_size`].
pub const MIN_STACK_SIZE: usize = 2048;
const PAINT: u8 = 0xa5;

/// Function that allocates backing memory for a coroutine [`Stack`] with the requested [`Layout`].
///
/// Must return either a null pointer on failure or a pointer to at least `layout.size()`
/// writable bytes aligned to `layout.align()`.
pub type StackAllocateFn = unsafe fn(Layout) -> *mut u8;

/// Function that releases backing memory previously returned by its paired [`StackAllocateFn`]
/// with the exact same [`Layout`].
pub type StackDeallocateFn = unsafe fn(*mut u8, Layout);

unsafe fn global_allocate(layout: Layout) -> *mut u8 {
    // SAFETY: forwarded to the global allocator with the caller's non-zero layout.
    unsafe { alloc(layout) }
}

unsafe fn global_deallocate(pointer: *mut u8, layout: Layout) {
    // SAFETY: `pointer` was returned by `global_allocate` with `layout`.
    unsafe { dealloc(pointer, layout) }
}

/// Allocator hooks used to obtain and release coroutine [`Stack`] backing memory.
///
/// By default, [`Stack`] uses [`StackAllocator::GLOBAL`], which forwards to
/// [`alloc::alloc::alloc`] and [`alloc::alloc::dealloc`]. Platforms whose global heap
/// places large allocations in memory that cannot safely back stacks (such as PSRAM on
/// Espressif boards, which becomes inaccessible during flash writes) can install an
/// allocator backed by always-accessible internal SRAM via [`set_stack_allocator`].
#[derive(Clone, Copy, Debug)]
pub struct StackAllocator {
    /// Allocate backing memory for a stack with the requested [`Layout`].
    pub allocate: StackAllocateFn,
    /// Release backing memory previously returned by [`Self::allocate`] with the same [`Layout`].
    pub deallocate: StackDeallocateFn,
}

impl StackAllocator {
    /// Create a stack allocator from an `allocate`/`deallocate` function-pointer pair.
    pub const fn new(allocate: StackAllocateFn, deallocate: StackDeallocateFn) -> Self {
        Self {
            allocate,
            deallocate,
        }
    }

    /// The default stack allocator backed by [`alloc::alloc::alloc`] and [`alloc::alloc::dealloc`].
    pub const GLOBAL: Self = Self {
        allocate: global_allocate,
        deallocate: global_deallocate,
    };
}

#[cfg(target_os = "linux")]
static STACK_ALLOCATOR: std::sync::Mutex<StackAllocator> =
    std::sync::Mutex::new(StackAllocator::GLOBAL);

#[cfg(not(target_os = "linux"))]
static STACK_ALLOCATOR: critical_section::Mutex<core::cell::Cell<StackAllocator>> =
    critical_section::Mutex::new(core::cell::Cell::new(StackAllocator::GLOBAL));

fn current_stack_allocator() -> StackAllocator {
    #[cfg(target_os = "linux")]
    {
        *STACK_ALLOCATOR
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    #[cfg(not(target_os = "linux"))]
    {
        critical_section::with(|cs| STACK_ALLOCATOR.borrow(cs).get())
    }
}

/// Register the allocator used for subsequent coroutine [`Stack`] allocations.
///
/// Platforms whose global allocator places large allocations in external memory that cannot
/// safely back stacks (such as PSRAM on Espressif boards, which is unreachable while writing
/// flash) should call this once during start-up before launching coroutines.
///
/// Each [`Stack`] stores the `deallocate` function from the allocator that created it, so
/// already-live stacks are always released through their matching deallocator even if the
/// hook is replaced afterwards. Pass [`StackAllocator::GLOBAL`] to restore the default
/// global allocator.
///
/// # Safety
/// - `allocator.allocate` must return either a null pointer or a non-null pointer to an
///   exclusive, writable memory block of at least `layout.size()` bytes aligned to
///   `layout.align()`.
/// - Any returned memory must remain valid and accessible (including across interrupts and
///   flash writes on bare-metal targets) until released via `allocator.deallocate`.
/// - `allocator.deallocate` must safely release any non-null pointer returned by
///   `allocator.allocate` when passed the exact matching `layout`.
/// - Both functions must be safe to call from any thread or core that creates or drops a
///   [`Stack`], without requiring the caller to hold a critical section.
pub unsafe fn set_stack_allocator(allocator: StackAllocator) {
    #[cfg(target_os = "linux")]
    {
        *STACK_ALLOCATOR
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = allocator;
    }
    #[cfg(not(target_os = "linux"))]
    {
        critical_section::with(|cs| STACK_ALLOCATOR.borrow(cs).set(allocator));
    }
}

/// Single heap allocation, aligned to 16 bytes, with a 64-byte painted lower guard.
/// This detects damage at boundaries; it does not provide memory protection.
pub struct Stack {
    memory: NonNull<u8>,
    layout: Layout,
    deallocate: StackDeallocateFn,
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
        let allocator = current_stack_allocator();
        // SAFETY: `layout` has non-zero size (at least MIN_STACK_SIZE + GUARD) and valid
        // 16-byte alignment; `allocator` satisfies the `set_stack_allocator` contract.
        let memory = NonNull::new(unsafe { (allocator.allocate)(layout) })?;
        unsafe { memory.as_ptr().write_bytes(PAINT, layout.size()) };
        Some(Self {
            memory,
            layout,
            deallocate: allocator.deallocate,
        })
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

// SAFETY: the allocation is 16-byte aligned and stable for the lifetime of the value.
// It deviates from two documented requirements of corosensei's `Stack` trait, neither of
// which corosensei checks for a custom stack (only its built-in stacks use them):
// - No guard page. Microcontrollers have no MMU to provide one, and on hosts the heap
//   block has none either. The painted guard only detects an overflow at the next switch.
// - At least 2 KiB of usable memory instead of corosensei's `MIN_STACK_SIZE` (4 KiB).
// Both are covered by the caller of the unsafe `Coroutine::new`, which must size the
// stack for all native stack use: the entry, its callees and interrupt frames (on
// Xtensa, `xtensa-lx-rt` pushes 256 bytes per interrupt level on the coroutine stack).
// Revisit this if a corosensei feature that relies on either requirement is enabled.
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
        // SAFETY: `self.memory` was allocated by the allocator paired with `self.deallocate`
        // using `self.layout`, and this `Stack` owns it exclusively until dropped.
        unsafe { (self.deallocate)(self.memory.as_ptr(), self.layout) };
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, RwLock};

    /// Serializes tests that mutate the global stack allocator hook against tests
    /// that rely on the default global allocator.
    static ALLOCATOR_TEST_LOCK: RwLock<()> = RwLock::new(());

    struct HookGuard<'a> {
        _lock: std::sync::RwLockWriteGuard<'a, ()>,
    }

    impl<'a> HookGuard<'a> {
        fn install(allocator: StackAllocator) -> Self {
            let lock = ALLOCATOR_TEST_LOCK
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            // SAFETY: test hooks satisfy the `StackAllocator` contract.
            unsafe { set_stack_allocator(allocator) };
            Self { _lock: lock }
        }
    }

    impl Drop for HookGuard<'_> {
        fn drop(&mut self) {
            // SAFETY: restoring the built-in global allocator is always valid.
            unsafe { set_stack_allocator(StackAllocator::GLOBAL) };
        }
    }

    #[derive(Default)]
    struct RecordedCalls {
        alloc_layouts: std::vec::Vec<(usize, Layout)>,
        dealloc_calls: std::vec::Vec<(usize, Layout)>,
    }

    static CUSTOM_A_STATE: Mutex<RecordedCalls> = Mutex::new(RecordedCalls {
        alloc_layouts: std::vec::Vec::new(),
        dealloc_calls: std::vec::Vec::new(),
    });

    static CUSTOM_B_STATE: Mutex<RecordedCalls> = Mutex::new(RecordedCalls {
        alloc_layouts: std::vec::Vec::new(),
        dealloc_calls: std::vec::Vec::new(),
    });

    static NULL_ALLOC_STATE: Mutex<(usize, usize)> = Mutex::new((0, 0));

    unsafe fn custom_a_allocate(layout: Layout) -> *mut u8 {
        let pointer = unsafe { alloc(layout) };
        if !pointer.is_null() {
            CUSTOM_A_STATE
                .lock()
                .unwrap()
                .alloc_layouts
                .push((pointer as usize, layout));
        }
        pointer
    }

    unsafe fn custom_a_deallocate(pointer: *mut u8, layout: Layout) {
        CUSTOM_A_STATE
            .lock()
            .unwrap()
            .dealloc_calls
            .push((pointer as usize, layout));
        unsafe { dealloc(pointer, layout) };
    }

    unsafe fn custom_b_allocate(layout: Layout) -> *mut u8 {
        let pointer = unsafe { alloc(layout) };
        if !pointer.is_null() {
            CUSTOM_B_STATE
                .lock()
                .unwrap()
                .alloc_layouts
                .push((pointer as usize, layout));
        }
        pointer
    }

    unsafe fn custom_b_deallocate(pointer: *mut u8, layout: Layout) {
        CUSTOM_B_STATE
            .lock()
            .unwrap()
            .dealloc_calls
            .push((pointer as usize, layout));
        unsafe { dealloc(pointer, layout) };
    }

    unsafe fn null_allocate(_layout: Layout) -> *mut u8 {
        NULL_ALLOC_STATE.lock().unwrap().0 += 1;
        core::ptr::null_mut()
    }

    unsafe fn null_deallocate(_pointer: *mut u8, _layout: Layout) {
        NULL_ALLOC_STATE.lock().unwrap().1 += 1;
    }

    #[test]
    fn detects_guard_damage() {
        let _guard = ALLOCATOR_TEST_LOCK
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let _guard = ALLOCATOR_TEST_LOCK
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let _guard = ALLOCATOR_TEST_LOCK
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stack = Stack::try_with_min_size(MIN_STACK_SIZE + 1, MIN_STACK_SIZE).unwrap();
        assert_eq!(stack.usable_size(), MIN_STACK_SIZE + 16);
        assert!(stack.guard_intact());
        // Unrepresentable and unobtainable sizes.
        assert!(Stack::try_with_min_size(usize::MAX, MIN_STACK_SIZE).is_none());
        assert!(Stack::try_with_min_size(isize::MAX as usize - 80, MIN_STACK_SIZE).is_none());
    }

    #[test]
    fn custom_allocator_allocates_and_matches_deallocator_across_hook_changes() {
        let _hook = HookGuard::install(StackAllocator::new(custom_a_allocate, custom_a_deallocate));
        {
            let mut a = CUSTOM_A_STATE.lock().unwrap();
            a.alloc_layouts.clear();
            a.dealloc_calls.clear();
            let mut b = CUSTOM_B_STATE.lock().unwrap();
            b.alloc_layouts.clear();
            b.dealloc_calls.clear();
        }

        let stack_a = Stack::try_with_min_size(MIN_STACK_SIZE + 1, MIN_STACK_SIZE).unwrap();
        assert_eq!(stack_a.usable_size(), MIN_STACK_SIZE + 16);
        assert!(stack_a.guard_intact());
        let expected_layout_a = Layout::from_size_align(MIN_STACK_SIZE + 16 + GUARD, 16).unwrap();
        let ptr_a = stack_a.memory.as_ptr() as usize;

        // Swap to a second custom allocator while `stack_a` is still live.
        unsafe { set_stack_allocator(StackAllocator::new(custom_b_allocate, custom_b_deallocate)) };
        let stack_b = Stack::new(DEFAULT_MIN_STACK_SIZE);
        let expected_layout_b =
            Layout::from_size_align(DEFAULT_MIN_STACK_SIZE + GUARD, 16).unwrap();
        let ptr_b = stack_b.memory.as_ptr() as usize;

        // Dropping `stack_a` must still invoke `custom_a_deallocate` with the exact pointer and layout.
        drop(stack_a);
        {
            let a = CUSTOM_A_STATE.lock().unwrap();
            assert_eq!(a.alloc_layouts, [(ptr_a, expected_layout_a)]);
            assert_eq!(a.dealloc_calls, [(ptr_a, expected_layout_a)]);
            let b = CUSTOM_B_STATE.lock().unwrap();
            assert_eq!(b.alloc_layouts, [(ptr_b, expected_layout_b)]);
            assert!(b.dealloc_calls.is_empty());
        }

        // Dropping `stack_b` invokes `custom_b_deallocate` with its exact pointer and layout.
        drop(stack_b);
        {
            let b = CUSTOM_B_STATE.lock().unwrap();
            assert_eq!(b.dealloc_calls, [(ptr_b, expected_layout_b)]);
        }
    }

    #[test]
    fn custom_allocator_returning_null_yields_none_without_deallocating() {
        let _hook = HookGuard::install(StackAllocator::new(null_allocate, null_deallocate));
        *NULL_ALLOC_STATE.lock().unwrap() = (0, 0);

        assert!(Stack::try_with_min_size(DEFAULT_MIN_STACK_SIZE, MIN_STACK_SIZE).is_none());
        let (alloc_calls, dealloc_calls) = *NULL_ALLOC_STATE.lock().unwrap();
        assert_eq!(alloc_calls, 1);
        assert_eq!(dealloc_calls, 0);
    }

    #[test]
    fn default_allocator_fallback_works_after_reset() {
        let _hook = HookGuard::install(StackAllocator::new(null_allocate, null_deallocate));
        *NULL_ALLOC_STATE.lock().unwrap() = (0, 0);

        unsafe { set_stack_allocator(StackAllocator::GLOBAL) };
        let stack = Stack::try_with_min_size(DEFAULT_MIN_STACK_SIZE, MIN_STACK_SIZE).unwrap();
        assert_eq!(stack.usable_size(), DEFAULT_MIN_STACK_SIZE);
        assert!(stack.guard_intact());
        drop(stack);

        assert_eq!(*NULL_ALLOC_STATE.lock().unwrap(), (0, 0));
    }
}
