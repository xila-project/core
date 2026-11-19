//! A memory manager backed by the standard system allocator.

use core::{cell::Cell, ptr::NonNull};
use memory::{CapabilityFlags, Layout, ManagerTrait};
use std::{
    alloc::{GlobalAlloc, System},
    sync::atomic::{AtomicUsize, Ordering},
};

/// Allocation counters recorded for a single thread by [`MemoryManager`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreadAllocations {
    allocations: usize,
    allocated_bytes: usize,
    deallocated_bytes: usize,
}

impl ThreadAllocations {
    /// Number of allocations performed on the thread.
    pub const fn allocations(self) -> usize {
        self.allocations
    }

    /// Total bytes allocated on the thread (not reduced by deallocations).
    pub const fn allocated_bytes(self) -> usize {
        self.allocated_bytes
    }

    /// Total bytes deallocated on the thread.
    pub const fn deallocated_bytes(self) -> usize {
        self.deallocated_bytes
    }

    /// Net bytes allocated minus deallocated on the thread.
    ///
    /// Can be negative if the thread freed memory allocated on another thread.
    pub const fn net_bytes(self) -> isize {
        self.allocated_bytes.wrapping_sub(self.deallocated_bytes) as isize
    }

    /// Counters accumulated between `earlier` and `self`.
    pub const fn since(self, earlier: Self) -> Self {
        Self {
            allocations: self.allocations.wrapping_sub(earlier.allocations),
            allocated_bytes: self.allocated_bytes.wrapping_sub(earlier.allocated_bytes),
            deallocated_bytes: self
                .deallocated_bytes
                .wrapping_sub(earlier.deallocated_bytes),
        }
    }
}

std::thread_local! {
    static THREAD_ALLOCATIONS: Cell<ThreadAllocations> = const {
        Cell::new(ThreadAllocations {
            allocations: 0,
            allocated_bytes: 0,
            deallocated_bytes: 0,
        })
    };
}

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

    /// Snapshot the allocation counters for the calling thread.
    pub fn thread_allocations() -> ThreadAllocations {
        THREAD_ALLOCATIONS.with(Cell::get)
    }
}

impl ManagerTrait for MemoryManager {
    unsafe fn allocate(&self, _: CapabilityFlags, layout: Layout) -> Option<NonNull<u8>> {
        let pointer = NonNull::new(unsafe { System.alloc(layout) })?;
        self.used.fetch_add(layout.size(), Ordering::Relaxed);
        let _ = THREAD_ALLOCATIONS.try_with(|counters| {
            let mut current = counters.get();
            current.allocations = current.allocations.wrapping_add(1);
            current.allocated_bytes = current.allocated_bytes.wrapping_add(layout.size());
            counters.set(current);
        });
        Some(pointer)
    }

    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        unsafe { System.dealloc(pointer.as_ptr(), layout) };
        self.used.fetch_sub(layout.size(), Ordering::Relaxed);
        let _ = THREAD_ALLOCATIONS.try_with(|counters| {
            let mut current = counters.get();
            current.deallocated_bytes = current.deallocated_bytes.wrapping_add(layout.size());
            counters.set(current);
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_counters_track_current_thread_and_ignore_other_threads() {
        let manager = MemoryManager::new();
        let layout = Layout::from_size_align(128, 8).unwrap();

        let before = MemoryManager::thread_allocations();
        let pointer = unsafe { manager.allocate(CapabilityFlags::empty(), layout) }.unwrap();
        let during = MemoryManager::thread_allocations().since(before);
        assert_eq!(during.allocations(), 1);
        assert_eq!(during.allocated_bytes(), 128);
        assert_eq!(during.deallocated_bytes(), 0);
        assert_eq!(during.net_bytes(), 128);

        let before_other_thread = MemoryManager::thread_allocations();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let other_before = MemoryManager::thread_allocations();
                let other = unsafe { manager.allocate(CapabilityFlags::empty(), layout) }.unwrap();
                unsafe { manager.deallocate(other, layout) };
                let other_delta = MemoryManager::thread_allocations().since(other_before);
                assert_eq!(other_delta.allocations(), 1);
                assert_eq!(other_delta.allocated_bytes(), 128);
                assert_eq!(other_delta.deallocated_bytes(), 128);
                assert_eq!(other_delta.net_bytes(), 0);
            });
        });
        assert_eq!(
            MemoryManager::thread_allocations().since(before_other_thread),
            ThreadAllocations::default(),
            "allocations on another thread must not affect this thread's counters"
        );

        unsafe { manager.deallocate(pointer, layout) };
        let after = MemoryManager::thread_allocations().since(before);
        assert_eq!(after.allocations(), 1);
        assert_eq!(after.allocated_bytes(), 128);
        assert_eq!(after.deallocated_bytes(), 128);
        assert_eq!(after.net_bytes(), 0);
        assert_eq!(manager.get_used(), 0);
    }

    #[test]
    fn cross_thread_deallocation_does_not_underflow() {
        let manager = MemoryManager::new();
        let layout = Layout::from_size_align(64, 8).unwrap();
        let pointer_address = unsafe { manager.allocate(CapabilityFlags::empty(), layout) }
            .unwrap()
            .as_ptr() as usize;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                let before = MemoryManager::thread_allocations();
                let pointer = NonNull::new(pointer_address as *mut u8).unwrap();
                unsafe { manager.deallocate(pointer, layout) };
                let delta = MemoryManager::thread_allocations().since(before);
                assert_eq!(delta.allocations(), 0);
                assert_eq!(delta.allocated_bytes(), 0);
                assert_eq!(delta.deallocated_bytes(), 64);
                assert_eq!(delta.net_bytes(), -64);
            });
        });
        assert_eq!(manager.get_used(), 0);
    }
}
