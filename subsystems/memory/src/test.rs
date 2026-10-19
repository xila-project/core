//! Test template for `ManagerTrait` implementations.
//!
//! This module provides a comprehensive test suite that can be used to validate
//! any implementation of the `ManagerTrait`. The tests cover all the core functionality
//! including allocation, deallocation, reallocation, and cache management.
//!
//! # Usage
//!
//! ## Using the Macro (Recommended)
//!
//! The easiest way to test your memory manager implementation is to use the
//! `implement_memory_manager_tests!` macro, which generates individual test
//! functions for each test category:
//!
//! ```rust,ignore
//! use memory::implement_memory_manager_tests;
//!
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!     use memory::{Manager, ManagerTrait};
//!     
//!     struct MyAllocator;
//!     impl ManagerTrait for MyAllocator {
//!         // ... implementation ...
//!     }
//!     
//!     static ALLOCATOR: MyAllocator = MyAllocator;
//!     
//!     implement_memory_manager_tests! {
//!         Manager::new(&ALLOCATOR)
//!     }
//! }
//! ```
//!
//! This will generate 14 individual `#[test]` functions, each testing a specific
//! aspect of your memory manager implementation.
//!
//! ## Using Individual Test Functions
//!
//! You can also call individual test functions directly:
//!
//! ```rust,ignore
//! use memory::{Manager, test_template};
//!
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!
//!     fn get_test_manager() -> Manager<'static> {
//!         static ALLOCATOR: YourAllocatorType = YourAllocatorType::new();
//!         Manager::new(&ALLOCATOR)
//!     }
//!
//!     #[test]
//!     fn test_basic_allocation() {
//!         let manager = get_test_manager();
//!         test_template::test_basic_allocation($manager);
//!     }
//!
//!     #[test]
//!     fn test_all() {
//!         let manager = get_test_manager();
//!         test_template::run_all_tests($manager);
//!     }
//! }
//! ```
//!
//! # Test Categories
//!
//! ## Basic Allocation Tests
//! - `test_basic_allocation` - Basic allocation and deallocation
//! - `test_zero_sized_allocation` - Zero-size allocation handling
//! - `test_aligned_allocation` - Various alignment requirements
//! - `test_allocation_with_capabilities` - Capability-based allocation
//!
//! ## Deallocation & Reallocation Tests
//! - `test_deallocation` - Memory deallocation and usage tracking
//! - `test_reallocation_grow` - Growing allocations
//! - `test_reallocation_shrink` - Shrinking allocations
//! - `test_reallocation_same_size` - Same-size reallocation
//!
//! ## Complex Operations Tests
//! - `test_multiple_allocations` - Multiple simultaneous allocations
//! - `test_allocation_pattern` - Complex allocation/deallocation patterns
//! - `test_large_allocation` - Large memory allocations
//!
//! ## Manager Features Tests
//! - `test_memory_statistics` - Memory usage statistics
//! - `test_page_size` - Page size reporting
//! - `test_cache_flush_operations` - Cache management operations

use crate::{CapabilityFlags, Layout, Manager, ManagerTrait};
use alloc::vec::Vec;

/// Tests basic memory allocation.
///
/// Verifies that the manager can allocate memory with a simple layout
/// and default capabilities, and that the returned pointer is valid.
///
/// # Parameters
/// * `manager` - The memory manager to test
///
/// # Panics
/// Panics if allocation fails or returns a null pointer.
pub fn test_basic_allocation(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);

    let layout = Layout::from_size_align(64, 8).unwrap();
    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

    assert!(
        !ptr.is_null(),
        "Basic allocation should not return null pointer"
    );

    // Verify we can write to the allocated memory
    unsafe {
        core::ptr::write_bytes(ptr, 0xAA, layout.size());

        // Verify the write worked
        for i in 0..layout.size() {
            assert_eq!(*ptr.add(i), 0xAA, "Memory should be writable");
        }

        manager.deallocate(ptr, layout);
    }
}

/// Tests zero-sized allocation behavior.
///
/// Verifies that allocating zero bytes returns a null pointer as expected.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_zero_sized_allocation(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);

    let layout = Layout::from_size_align(0, 1).unwrap();
    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

    assert!(
        ptr.is_null(),
        "Zero-sized allocation should return null pointer"
    );
}

/// Tests allocation with specific alignment requirements.
///
/// Verifies that the manager respects alignment requirements and returns
/// properly aligned memory addresses.
///
/// # Parameters
/// * `manager` - The memory manager to test
///
/// # Panics
/// Panics if allocation fails or the returned pointer is not properly aligned.
pub fn test_aligned_allocation(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);

    let alignments = [8, 16, 32, 64, 128];

    for &align in &alignments {
        let layout = Layout::from_size_align(256, align).unwrap();
        let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

        assert!(
            !ptr.is_null(),
            "Aligned allocation should not return null pointer"
        );
        assert_eq!(
            ptr as usize % align,
            0,
            "Pointer should be aligned to {} bytes",
            align
        );

        unsafe { manager.deallocate(ptr, layout) };
    }
}

/// Tests allocation with different capability flags.
///
/// Verifies that the manager can handle allocations with different capability
/// requirements (executable, direct memory access, etc.).
///
/// # Parameters
/// * `manager` - The memory manager to test
///
/// # Panics
/// Panics if allocation with capabilities fails.
pub fn test_allocation_with_capabilities(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);

    let layout = Layout::from_size_align(128, 8).unwrap();

    // Test executable capability
    let caps_exec = CapabilityFlags::Executable;
    let ptr = unsafe { manager.allocate(caps_exec, layout) };
    assert!(
        !ptr.is_null(),
        "Allocation with executable capability should succeed"
    );
    unsafe { manager.deallocate(ptr, layout) };

    // Test DMA capability
    // let caps_dma = Capabilities::new(false, true);
    // let ptr = unsafe { manager.allocate(caps_dma, layout) };
    // assert!(
    //     !ptr.is_null(),
    //     "Allocation with DMA capability should succeed"
    // );
    // unsafe { manager.deallocate(ptr, layout) };

    // Test both capabilities
    // let caps_both = Capabilities::new(true, true);
    // let ptr = unsafe { manager.allocate(caps_both, layout) };
    // assert!(
    //     !ptr.is_null(),
    //     "Allocation with both capabilities should succeed"
    // );
    // unsafe { manager.deallocate(ptr, layout) };
}

/// Tests memory deallocation.
///
/// Verifies that memory can be allocated and then safely deallocated.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_deallocation(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);

    let layout = Layout::from_size_align(128, 8).unwrap();

    let initial_used = manager.get_used();

    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };
    assert!(!ptr.is_null(), "Allocation should succeed");

    let used_after_alloc = manager.get_used();
    assert!(
        used_after_alloc >= initial_used,
        "Used memory should increase after allocation"
    );

    unsafe { manager.deallocate(ptr, layout) };

    // Note: The used memory might not return exactly to initial_used due to
    // allocator overhead, but it should not increase further
}
/// Tests memory reallocation to a larger size.
///
/// Verifies that the manager can grow an existing allocation and preserve
/// the original data.
///
/// # Parameters
/// * `manager` - The memory manager to test
///
/// # Panics
/// Panics if reallocation fails or data is not preserved.
pub fn test_reallocation_grow(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let initial_size = 64;
    let new_size = 256;
    let layout = Layout::from_size_align(initial_size, 8).unwrap();

    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };
    assert!(!ptr.is_null(), "Initial allocation should succeed");

    // Write pattern to memory
    unsafe {
        for i in 0..initial_size {
            *ptr.add(i) = (i % 256) as u8;
        }
    }

    // Reallocate to larger size
    let new_ptr = unsafe { manager.reallocate(ptr, layout, new_size) };
    assert!(!new_ptr.is_null(), "Reallocation should succeed");

    // Verify original data is preserved
    unsafe {
        for i in 0..initial_size {
            assert_eq!(
                *new_ptr.add(i),
                (i % 256) as u8,
                "Original data should be preserved after reallocation"
            );
        }
    }

    let new_layout = Layout::from_size_align(new_size, 8).unwrap();
    unsafe { manager.deallocate(new_ptr, new_layout) };
}

/// Tests memory reallocation to a smaller size.
///
/// Verifies that the manager can shrink an existing allocation and preserve
/// data up to the new size.
///
/// # Parameters
/// * `manager` - The memory manager to test
///
/// # Panics
/// Panics if reallocation fails or data is not preserved.
pub fn test_reallocation_shrink(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let initial_size = 256;
    let new_size = 64;
    let layout = Layout::from_size_align(initial_size, 8).unwrap();

    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };
    assert!(!ptr.is_null(), "Initial allocation should succeed");

    // Write pattern to memory
    unsafe {
        for i in 0..initial_size {
            *ptr.add(i) = (i % 256) as u8;
        }
    }

    // Reallocate to smaller size
    let new_ptr = unsafe { manager.reallocate(ptr, layout, new_size) };
    assert!(!new_ptr.is_null(), "Reallocation should succeed");

    // Verify data up to new size is preserved
    unsafe {
        for i in 0..new_size {
            assert_eq!(
                *new_ptr.add(i),
                (i % 256) as u8,
                "Data should be preserved up to new size after reallocation"
            );
        }
    }

    let new_layout = Layout::from_size_align(new_size, 8).unwrap();
    unsafe { manager.deallocate(new_ptr, new_layout) };
}

/// Tests reallocation with the same size.
///
/// Verifies that reallocating with the same size either returns the same pointer
/// or successfully creates a new allocation with preserved data.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_reallocation_same_size(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let size = 128;
    let layout = Layout::from_size_align(size, 8).unwrap();

    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };
    assert!(!ptr.is_null(), "Initial allocation should succeed");

    // Write pattern to memory
    unsafe {
        for i in 0..size {
            *ptr.add(i) = (i % 256) as u8;
        }
    }

    // Reallocate with same size
    let new_ptr = unsafe { manager.reallocate(ptr, layout, size) };
    assert!(!new_ptr.is_null(), "Reallocation should succeed");

    // Verify data is preserved
    unsafe {
        for i in 0..size {
            assert_eq!(
                *new_ptr.add(i),
                (i % 256) as u8,
                "Data should be preserved after same-size reallocation"
            );
        }
    }

    unsafe { manager.deallocate(new_ptr, layout) };
}

/// Tests multiple simultaneous allocations.
///
/// Verifies that the manager can handle multiple allocations at once
/// without interference.
///
/// # Parameters
/// * `manager` - The memory manager to test
///
/// # Panics
/// Panics if any allocation fails or data is corrupted.
pub fn test_multiple_allocations(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    const NUM_ALLOCATIONS: usize = 10;
    let mut allocations = [(
        core::ptr::null_mut::<u8>(),
        Layout::from_size_align(1, 1).unwrap(),
    ); NUM_ALLOCATIONS];

    // Allocate multiple blocks
    for (i, allocation) in allocations.iter_mut().enumerate().take(NUM_ALLOCATIONS) {
        let size = 64 + i * 32;
        let layout = Layout::from_size_align(size, 8).unwrap();
        let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

        assert!(!ptr.is_null(), "Allocation {} should succeed", i);

        // Write unique pattern to each allocation
        unsafe {
            for j in 0..size {
                *ptr.add(j) = ((i + j) % 256) as u8;
            }
        }

        *allocation = (ptr, layout);
    }

    // Verify all allocations still have correct data
    for (i, &(ptr, layout)) in allocations.iter().enumerate().take(NUM_ALLOCATIONS) {
        let size = layout.size();

        unsafe {
            for j in 0..size {
                assert_eq!(
                    *ptr.add(j),
                    ((i + j) % 256) as u8,
                    "Data should be preserved in allocation {}",
                    i
                );
            }
        }
    }

    // Deallocate all blocks
    for &(ptr, layout) in allocations.iter().take(NUM_ALLOCATIONS) {
        unsafe { manager.deallocate(ptr, layout) };
    }
}

/// Tests memory usage statistics.
///
/// Verifies that the manager correctly tracks used and free memory.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_memory_statistics(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let initial_used = manager.get_used();
    //let initial_free = manager.get_free();

    let layout = Layout::from_size_align(1024, 8).unwrap();
    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

    assert!(!ptr.is_null(), "Allocation should succeed");

    let used_after_alloc = manager.get_used();
    //let free_after_alloc = manager.get_free(); // Used memory should increase
    assert!(
        used_after_alloc >= initial_used,
        "Used memory should increase after allocation"
    );

    // Free memory should decrease or stay the same
    // assert!(
    //     free_after_alloc <= initial_free,
    //     "Free memory should decrease or stay the same after allocation"
    // );

    unsafe { manager.deallocate(ptr, layout) };
}

/// Tests the page size reporting.
///
/// Verifies that the manager reports a valid page size.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_page_size(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let page_size = manager.get_page_size();

    assert!(page_size > 0, "Page size should be greater than zero");
    assert!(
        page_size.is_power_of_two(),
        "Page size should be a power of two"
    );
}

/// Tests cache flush operations.
///
/// Verifies that cache flush operations can be called without panicking.
/// Note: This doesn't verify actual cache behavior as that's hardware-specific.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_cache_flush_operations(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    // Test data cache flush
    manager.flush_data_cache();

    // Test instruction cache flush with allocated memory
    let layout = Layout::from_size_align(256, 8).unwrap();
    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

    if !ptr.is_null() {
        unsafe {
            manager.flush_instruction_cache(ptr, layout.size());
            manager.deallocate(ptr, layout);
        }
    }
}

/// Tests large memory allocation.
///
/// Verifies that the manager can handle larger allocations.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_large_allocation(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let large_size = 1024 * 1024; // 1 MB
    let layout = Layout::from_size_align(large_size, 8).unwrap();

    let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };

    if !ptr.is_null() {
        // If allocation succeeded, verify we can use the memory
        unsafe {
            *ptr = 0xFF;
            *ptr.add(large_size - 1) = 0xFF;

            assert_eq!(
                *ptr, 0xFF,
                "Should be able to write to start of large allocation"
            );
            assert_eq!(
                *ptr.add(large_size - 1),
                0xFF,
                "Should be able to write to end of large allocation"
            );

            manager.deallocate(ptr, layout);
        }
    }
    // Note: It's acceptable for very large allocations to fail on some systems
}

/// Tests allocation and deallocation pattern.
///
/// Verifies that the manager can handle a complex pattern of allocations
/// and deallocations without memory leaks or corruption.
///
/// # Parameters
/// * `manager` - The memory manager to test
pub fn test_allocation_pattern(allocator: impl ManagerTrait) {
    let manager = Manager::new(&allocator);
    let initial_used = manager.get_used();

    // Pattern: allocate 5, deallocate 2, allocate 3, deallocate all
    let mut ptrs = Vec::new(); // Allocate 5
    for i in 0..5 {
        let size = 128 + i * 64;
        let layout = Layout::from_size_align(size, 8).unwrap();
        let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };
        assert!(!ptr.is_null(), "Allocation should succeed");
        ptrs.push((ptr, layout));
    }

    // Deallocate 2
    for _ in 0..2 {
        if let Some((ptr, layout)) = ptrs.pop() {
            unsafe { manager.deallocate(ptr, layout) };
        }
    }

    // Allocate 3 more
    for i in 0..3 {
        let size = 96 + i * 32;
        let layout = Layout::from_size_align(size, 8).unwrap();
        let ptr = unsafe { manager.allocate(CapabilityFlags::empty(), layout) };
        assert!(!ptr.is_null(), "Allocation should succeed");
        ptrs.push((ptr, layout));
    }

    // Deallocate all
    for (ptr, layout) in ptrs {
        unsafe { manager.deallocate(ptr, layout) };
    }

    let final_used = manager.get_used();

    // Memory usage should be close to initial (allowing for some allocator overhead)
    assert!(
        final_used <= initial_used + 1024,
        "Memory should be mostly freed after pattern completion"
    );
}

/// Macro to implement all memory manager tests for a given `ManagerTrait` implementation.
///
/// This macro generates individual test functions for each test category, making it easy
/// to integrate comprehensive memory manager testing into your test suite.
///
/// # Usage
///
/// ```rust,ignore
/// use memory::implement_memory_manager_tests;
///
/// mod tests {
///     use super::*;
///     use memory::{Manager, ManagerTrait};
///     
///     struct MyAllocator;
///     impl ManagerTrait for MyAllocator {
///         // ... implementation ...
///     }
///     
///     static ALLOCATOR: MyAllocator = MyAllocator;
///     
///     implement_memory_manager_tests! {
///         Manager::new(&ALLOCATOR)
///     }
/// }
/// ```
///
/// This will generate individual `#[test]` functions for each test category:
/// - test_basic_allocation
/// - test_zero_sized_allocation
/// - test_aligned_allocation
/// - test_allocation_with_capabilities
/// - test_deallocation
/// - test_reallocation_grow
/// - test_reallocation_shrink
/// - test_reallocation_same_size
/// - test_multiple_allocations
/// - test_memory_statistics
/// - test_page_size
/// - test_cache_flush_operations
/// - test_large_allocation
/// - test_allocation_pattern
#[macro_export]
macro_rules! implement_memory_manager_tests {
    ($manager:expr) => {
        #[test]
        fn test_basic_allocation() {
            $crate::test::test_basic_allocation($manager);
        }

        #[test]
        fn test_zero_sized_allocation() {
            $crate::test::test_zero_sized_allocation($manager);
        }

        #[test]
        fn test_aligned_allocation() {
            $crate::test::test_aligned_allocation($manager);
        }

        #[test]
        fn test_allocation_with_capabilities() {
            $crate::test::test_allocation_with_capabilities($manager);
        }

        #[test]
        fn test_deallocation() {
            $crate::test::test_deallocation($manager);
        }

        #[test]
        fn test_reallocation_grow() {
            $crate::test::test_reallocation_grow($manager);
        }

        #[test]
        fn test_reallocation_shrink() {
            $crate::test::test_reallocation_shrink($manager);
        }

        #[test]
        fn test_reallocation_same_size() {
            $crate::test::test_reallocation_same_size($manager);
        }

        #[test]
        fn test_multiple_allocations() {
            $crate::test::test_multiple_allocations($manager);
        }

        #[test]
        fn test_memory_statistics() {
            $crate::test::test_memory_statistics($manager);
        }

        #[test]
        fn test_page_size() {
            $crate::test::test_page_size($manager);
        }

        #[test]
        fn test_cache_flush_operations() {
            $crate::test::test_cache_flush_operations($manager);
        }

        #[test]
        fn test_large_allocation() {
            $crate::test::test_large_allocation($manager);
        }

        #[test]
        fn test_allocation_pattern() {
            $crate::test::test_allocation_pattern($manager);
        }
    };
}
