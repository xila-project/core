//! A memory manager backed by the standard system allocator.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use memory::{CapabilityFlags, Layout, ManagerTrait};
use std::alloc::{GlobalAlloc, System};

/// Xila memory manager for tests: every capability is served by [`System`].
pub struct MemoryManager {
    used: AtomicUsize,
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryManager {
    pub const fn new() -> Self {
        Self {
            used: AtomicUsize::new(0),
        }
    }
}

impl ManagerTrait for MemoryManager {
    unsafe fn allocate(&self, _: CapabilityFlags, layout: Layout) -> Option<NonNull<u8>> {
        let pointer = NonNull::new(unsafe { System.alloc(layout) })?;
        self.used.fetch_add(layout.size(), Ordering::Relaxed);
        Some(pointer)
    }

    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        unsafe { System.dealloc(pointer.as_ptr(), layout) };
        self.used.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    fn get_used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }

    fn get_free(&self) -> usize {
        (isize::MAX as usize).saturating_sub(self.get_used())
    }
}

/// Install [`MemoryManager`] as the global allocator and Xila memory manager.
#[macro_export]
macro_rules! instantiate_global_allocator {
    () => {
        static __XILA_TESTING_MEMORY_MANAGER: $crate::MemoryManager = $crate::MemoryManager::new();
        $crate::memory_crate::instantiate_global_allocator!(&__XILA_TESTING_MEMORY_MANAGER);
    };
}
