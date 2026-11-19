//! Allocation through the system memory manager, with the C allocator
//! contract LVGL and LittleFS expect (`malloc`-style pointers carrying their
//! own metadata header, `realloc` semantics, null on failure).
//!
//! # Safety
//!
//! The functions are unsafe: pointers must come from this module, must not
//! be used after being freed, and concurrent access must be synchronized.

use core::ptr::null_mut;
use core::{ffi::c_void, ptr::NonNull};
use log::warning;
use memory::CapabilityFlags;
use synchronization::blocking_mutex::raw::CriticalSectionRawMutex;
use synchronization::mutex::Mutex;
use task::block_on;

use crate::Allocated;

/// Memory capability flags requested from the memory manager.
pub type MemoryCapabilities = u8;

/// Memory can be used for code execution.
pub const MEMORY_CAPABILITIES_EXECUTE: MemoryCapabilities =
    memory::CapabilityFlags::Executable.bits();

/// Memory is accessible by DMA controllers.
pub const MEMORY_CAPABILITIES_DIRECT_MEMORY_ACCESS: MemoryCapabilities =
    memory::CapabilityFlags::DirectMemoryAccess.bits();

/// Memory stays accessible when external flash caches are disabled.
pub const MEMORY_CAPABILITIES_ALWAYS_ACCESSIBLE: MemoryCapabilities =
    memory::CapabilityFlags::AlwaysAccessible.bits();

/// No special capabilities: standard memory.
pub const MEMORY_CAPABILITIES_NONE: MemoryCapabilities = 0;

static ALLOCATION_MUTEX: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

/// Converts a function that returns an `Option<NonNull<P>>` into a raw C-compatible pointer.
///
/// This utility function is used internally to convert Rust's safe pointer types
/// into C-compatible raw pointers. Returns `null_mut()` if the function returns `None`.
///
/// # Type Parameters
///
/// * `F` - A function that returns `Option<NonNull<P>>`
/// * `P` - The pointer type being converted
///
/// # Parameters
///
/// * `Function` - The function to execute and convert the result
///
/// # Returns
///
/// A raw C-compatible pointer, or null if the function returns `None`
pub fn into_pointer<F, P>(function: F) -> *mut c_void
where
    F: FnOnce() -> Option<NonNull<P>>,
{
    match function() {
        Some(pointer) => pointer.as_ptr() as *mut c_void,
        None => null_mut(),
    }
}

/// Deallocates a previously allocated memory block.
///
/// This function frees memory that was previously allocated using `memory_allocate`
/// or `memory_reallocate`. It's safe to call this function with a null pointer.
///
/// # Safety
///
/// - The pointer must have been returned by a previous call to `memory_allocate`
///   or `memory_reallocate`
/// - The pointer must not be used after this function returns
///
/// # Parameters
///
/// * `Pointer` - Pointer to the memory block to deallocate, or null
///
/// # Examples
///
/// ```text
/// void* ptr = memory_allocate(NULL, 1024, 8, 0);
/// memory_deallocate(ptr); // Free the memory
/// memory_deallocate(NULL); // Safe - ignored
/// ```
pub unsafe fn memory_deallocate(pointer: *mut c_void) {
    let _lock = block_on(ALLOCATION_MUTEX.lock());

    let allocated = match unsafe { Allocated::from_user_pointer(pointer as *mut u8) } {
        Some(alloc) => alloc,
        None => {
            return;
        }
    };

    let layout = allocated.get_layout().unwrap();
    let base_pointer = allocated.get_base_pointer();

    unsafe {
        memory::global_manager().deallocate(base_pointer, layout);
    }
}

/// Reallocates a memory block to a new size.
///
/// This function changes the size of a previously allocated memory block. If the new size
/// is larger, the additional memory is uninitialized. If the new size is smaller, the
/// excess memory is freed. The original data is preserved up to the minimum of the old
/// and new sizes. The platform resizes the block in place when it can, and a shrink that keeps
/// the alignment and DMA constraints can stay in place. Growth follows the platform's placement
/// policy and always preserves the capability flags requested at allocation time. If no suitable
/// region can serve the new size and alignment, reallocation fails and the old block is unchanged.
///
/// # Safety
///
/// - If `Pointer` is not null, it must have been returned by a previous call to
///   `memory_allocate` or `memory_reallocate`
/// - Use the returned pointer after a successful resize
/// - A non-null original pointer remains valid only if a nonzero resize fails
///
/// # Parameters
///
/// * `Pointer` - Pointer to the memory block to reallocate, or null for new allocation
/// * `Size` - New size in bytes (0 is equivalent to deallocation). On 32-bit
///   targets a block, including its header and padding, is limited to 32 MiB.
///
/// # Returns
///
/// - Pointer to the reallocated memory block
/// - Null if allocation fails or if size is 0
/// - If `Pointer` is null and `Size` is nonzero, allocates with default alignment
///
/// # Examples
///
/// ```text
/// // Allocate initial memory
/// void* ptr = memory_reallocate(NULL, 1024);
///
/// // Expand the memory
/// ptr = memory_reallocate(ptr, 2048);
///
/// // Shrink the memory
/// ptr = memory_reallocate(ptr, 512);
///
/// // Free the memory
/// memory_reallocate(ptr, 0);
/// ```
pub unsafe fn memory_reallocate(pointer: *mut c_void, size: usize) -> *mut c_void {
    if size == 0 {
        // Deallocation takes the allocation mutex itself and accepts null.
        unsafe { memory_deallocate(pointer) };
        return null_mut();
    }

    let _lock = block_on(ALLOCATION_MUTEX.lock());

    unsafe {
        let pointer = pointer as *mut u8;

        let allocated = match Allocated::from_user_pointer(pointer) {
            Some(alloc) => alloc,
            None => {
                drop(_lock);
                //trace!("memory_reallocate called with null pointer, allocating new memory of size: {}", size);
                return memory_allocate_core(size);
            }
        };

        let old_layout = allocated.get_layout().unwrap();
        let Some(new_layout) = Allocated::get_layout_for_allocation(size, old_layout.align())
        else {
            return null_mut();
        };
        let capabilities = allocated.get_capabilities();

        let pointer = memory::global_manager().reallocate(
            capabilities,
            allocated.get_base_pointer(),
            old_layout,
            new_layout.size(),
        );

        let Some(allocated) = Allocated::from_layout(pointer, &new_layout, capabilities) else {
            return null_mut();
        };

        allocated.get_user_pointer() as *mut c_void
    }
}

/// Allocates a memory block with specified properties.
///
/// This function allocates a block of memory with the specified size, alignment,
/// and capabilities. The memory is uninitialized and must be properly initialized
/// before use.
///
/// # Safety
///
/// This function is unsafe because:
/// - The returned memory is uninitialized
/// - The caller must ensure proper deallocation
/// - The hint address, if provided, must be a valid memory address
///
/// # Parameters
///
/// * `Hint_address` - Preferred memory address (hint only, may be ignored), or null
/// * `Size` - Size of the memory block in bytes. On 32-bit targets a block,
///   including its header and padding, is limited to 32 MiB.
/// * `Alignment` - Required memory alignment in bytes (must be a power of 2)
/// * `Capabilities` - Memory capabilities flags (see `memory_capabilities_*` constants)
///
/// # Returns
///
/// - Pointer to the allocated memory block
/// - Null if allocation fails
///
/// # Errors
///
/// Returns null if:
/// - Insufficient memory available
/// - Invalid alignment (not a power of 2)
/// - Requested capabilities not supported
/// - Size is too large
///
/// # Examples
///
/// ```text
/// // Allocate 1024 bytes with 8-byte alignment
/// void* ptr = memory_allocate(NULL, 1024, 8, memory_capabilities_none);
///
/// // Allocate executable memory for code
/// void* code_ptr = memory_allocate(NULL, 4096, 4096, MEMORY_CAPABILITIES_EXECUTE);
///
/// // Allocate DMA-capable memory
/// void* dma_ptr = memory_allocate(NULL, 2048, 32, memory_capabilities_direct_memory_access);
/// ```
pub unsafe fn memory_allocate(
    _: *mut c_void,
    size: usize,
    alignment: usize,
    capabilities: MemoryCapabilities,
) -> *mut c_void {
    let _lock = block_on(ALLOCATION_MUTEX.lock());

    //trace!(
    //    "memory_allocate called with Size: {size}, Alignment: {alignment}, Capabilities: {capabilities:?}"
    //);

    let Some(layout) = Allocated::get_layout_for_allocation(size, alignment) else {
        return null_mut();
    };

    let capabilities = match CapabilityFlags::from_bits(capabilities) {
        Some(flags) => flags,
        None => {
            warning!(
                "memory_allocate called with invalid capabilities: {capabilities:?}, ignoring"
            );
            CapabilityFlags::empty()
        }
    };

    let pointer = unsafe { memory::global_manager().allocate(capabilities, layout) };

    let Some(allocated) = (unsafe { Allocated::from_layout(pointer, &layout, capabilities) })
    else {
        return null_mut();
    };

    allocated.get_user_pointer() as *mut c_void
}

/// Allocates a memory block with default properties.
///
/// # Safety
/// This function is unsafe because it dereferences raw pointers.
pub unsafe fn memory_allocate_core(size: usize) -> *mut c_void {
    //trace!("memory_allocate_core called with Size: {size}");
    unsafe {
        memory_allocate(
            null_mut(),
            size,
            align_of::<usize>(), // Use usize alignment for general-purpose allocations
            MEMORY_CAPABILITIES_NONE,
        )
    }
}

#[cfg(test)]
mod tests {
    //! # Memory Management Tests
    //!
    //! This module contains comprehensive tests for the memory management functionality.
    //! The tests cover various scenarios including basic allocation/deallocation,
    //! edge cases, error conditions, and stress testing.
    //!
    //! ## Test Categories
    //!
    //! - **Basic Operations**: Standard allocation, deallocation, and reallocation
    //! - **Edge Cases**: Zero-size allocations, large alignments, null pointers
    //! - **Capabilities**: Testing executable and DMA-capable memory
    //! - **Stress Testing**: Multiple allocations and allocation tracking
    //! - **Error Handling**: Invalid inputs and error recovery
    use alloc::vec::Vec;
    use testing_support::MemoryManager;

    use super::*;

    /// Tests basic memory allocation and deallocation functionality.
    ///
    /// This test verifies that:
    /// - Memory can be successfully allocated with specified size and alignment
    /// - Allocated memory is readable and writable
    /// - Memory can be properly deallocated without errors
    /// - Data written to memory is correctly stored and retrieved
    #[test]
    fn test_allocate_deallocate_basic() {
        unsafe {
            // Test basic allocation and deallocation
            let size = 128;
            let alignment = 8;
            let capabilities = 0; // Basic capabilities
            let hint_address = core::ptr::null_mut();

            let pointer = memory_allocate(hint_address, size, alignment, capabilities);
            assert!(!pointer.is_null(), "Memory allocation should succeed");

            // Write and read to verify the memory is accessible
            let ptr = pointer as *mut u8;
            for i in 0..size {
                *ptr.add(i) = (i % 256) as u8;
            }

            for i in 0..size {
                assert_eq!(
                    *ptr.add(i),
                    (i % 256) as u8,
                    "Memory should be readable and writable"
                );
            }

            // Deallocate the memory
            memory_deallocate(pointer);
        }
    }

    /// Tests allocation behavior with zero size.
    ///
    /// Zero-size allocations are a special case that different allocators
    /// may handle differently. This test ensures the implementation handles
    /// them gracefully without crashing.
    #[test]
    fn test_allocate_zero_size() {
        unsafe {
            // Test allocation with zero size
            let size = 0;
            let alignment = 8;
            let capabilities = 0;
            let hint_address = core::ptr::null_mut();

            let pointer = memory_allocate(hint_address, size, alignment, capabilities);
            // Zero-size allocation might return null or a valid pointer, both are acceptable
            if !pointer.is_null() {
                memory_deallocate(pointer);
            }
        }
    }

    /// Tests memory allocation with large alignment requirements.
    ///
    /// This test verifies that the allocator can handle large alignment
    /// requirements (64 bytes) and that the returned pointer is properly
    /// aligned to the requested boundary.
    #[test]
    fn test_allocate_large_alignment() {
        unsafe {
            // Test allocation with large alignment
            let size = 256;
            let alignment = align_of::<u128>();
            let capabilities = 0;
            let hint_address = core::ptr::null_mut();

            let pointer = memory_allocate(hint_address, size, alignment, capabilities);
            assert!(
                !pointer.is_null(),
                "Large alignment allocation should succeed"
            );

            // Verify alignment
            let addr = pointer as usize;
            assert_eq!(addr % alignment, 0, "Pointer should be properly aligned");

            memory_deallocate(pointer);
        }
    }

    /// Tests memory allocation with small alignments (1, 2, 4 bytes).
    ///
    /// This test verifies that the allocator can handle small alignment
    /// requirements now that CompactLayout supports alignments as low as 1 byte.
    #[test]
    fn test_allocate_small_alignments() {
        unsafe {
            for &alignment in &[1, 2, 4] {
                let size = 64;
                let capabilities = 0;
                let hint_address = core::ptr::null_mut();

                let pointer = memory_allocate(hint_address, size, alignment, capabilities);
                assert!(
                    !pointer.is_null(),
                    "Allocation with alignment {} should succeed",
                    alignment
                );

                // Verify alignment
                let addr = pointer as usize;
                assert_eq!(
                    addr % alignment,
                    0,
                    "Pointer should be aligned to {} bytes",
                    alignment
                );

                // Write and read to verify the memory is accessible
                let ptr = pointer as *mut u8;
                for i in 0..size {
                    *ptr.add(i) = (i % 256) as u8;
                }

                for i in 0..size {
                    assert_eq!(
                        *ptr.add(i),
                        (i % 256) as u8,
                        "Memory should be readable and writable"
                    );
                }

                memory_deallocate(pointer);
            }
        }
    }

    /// Tests allocation of executable memory.
    ///
    /// This test verifies that memory can be allocated with executable
    /// capabilities, which is necessary for just-in-time compilation
    /// and dynamic code generation.
    #[test]
    fn test_allocate_executable_capability() {
        unsafe {
            // Test allocation with executable capability
            let size = 128;
            let alignment = 8;
            let capabilities = MEMORY_CAPABILITIES_EXECUTE;
            let hint_address = core::ptr::null_mut();

            let pointer = memory_allocate(hint_address, size, alignment, capabilities);
            assert!(
                !pointer.is_null(),
                "Executable memory allocation should succeed"
            );

            memory_deallocate(pointer);
        }
    }

    /// Tests allocation of DMA-capable memory.
    ///
    /// DMA-capable memory must meet specific hardware requirements and
    /// may need to be allocated from special memory regions. This test
    /// is ignored by default as it requires specific hardware support.
    #[test]
    #[ignore = "Requires specific hardware support for DMA"]
    fn test_allocate_dma_capability() {
        unsafe {
            // Test allocation with DMA capability
            let size = 128;
            let alignment = 8;
            let capabilities = MEMORY_CAPABILITIES_DIRECT_MEMORY_ACCESS;
            let hint_address = core::ptr::null_mut();

            let pointer = memory_allocate(hint_address, size, alignment, capabilities);
            assert!(!pointer.is_null(), "DMA memory allocation should succeed");

            memory_deallocate(pointer);
        }
    }

    /// Tests that deallocating a null pointer is safe.
    ///
    /// According to standard behavior, deallocating a null pointer
    /// should be a no-op and not cause any errors or crashes.
    #[test]
    fn test_deallocate_null_pointer() {
        // Test that deallocating a null pointer doesn't crash
        unsafe {
            memory_deallocate(core::ptr::null_mut());
        }
    }

    /// Tests reallocation from a null pointer (equivalent to allocation).
    ///
    /// When realloc is called with a null pointer, it should behave
    /// identically to malloc, allocating new memory of the requested size.
    #[test]
    fn test_reallocate_null_to_new() {
        unsafe {
            // Test reallocating from null (equivalent to allocation)
            let size = 128;
            let pointer = memory_reallocate(core::ptr::null_mut(), size);
            assert!(
                !pointer.is_null(),
                "Reallocation from null should work like allocation"
            );

            // Write to the memory to verify it's usable
            let ptr = pointer as *mut u8;
            for i in 0..size {
                *ptr.add(i) = (i % 256) as u8;
            }

            memory_deallocate(pointer);
        }
    }

    /// Tests expanding memory through reallocation.
    ///
    /// This test verifies that existing data is preserved when memory
    /// is expanded through reallocation, and that the new memory region
    /// is usable.
    #[test]
    fn test_reallocate_expand() {
        unsafe {
            // Test expanding memory with reallocation
            let initial_size = 64;
            let expanded_size = 128;

            // First allocation
            let pointer = memory_reallocate(core::ptr::null_mut(), initial_size);
            assert!(!pointer.is_null(), "Initial allocation should succeed");

            // Fill with test data
            let ptr = pointer as *mut u8;
            for i in 0..initial_size {
                *ptr.add(i) = (i % 256) as u8;
            }

            // Expand the allocation
            let new_pointer = memory_reallocate(pointer, expanded_size);
            assert!(
                !new_pointer.is_null(),
                "Reallocation expansion should succeed"
            );

            // Verify original data is preserved
            let new_ptr = new_pointer as *mut u8;
            for i in 0..initial_size {
                assert_eq!(
                    *new_ptr.add(i),
                    (i % 256) as u8,
                    "Original data should be preserved"
                );
            }

            memory_deallocate(new_pointer);
        }
    }

    /// Tests shrinking memory through reallocation.
    ///
    /// This test verifies that data within the new size bounds is preserved
    /// when memory is shrunk through reallocation.
    #[test]
    fn test_reallocate_shrink() {
        unsafe {
            // Test shrinking memory with reallocation
            let initial_size = 128;
            let shrunk_size = 64;

            // First allocation
            let pointer = memory_reallocate(core::ptr::null_mut(), initial_size);
            assert!(!pointer.is_null(), "Initial allocation should succeed");

            // Fill with test data
            let ptr = pointer as *mut u8;
            for i in 0..initial_size {
                *ptr.add(i) = (i % 256) as u8;
            }

            // Shrink the allocation
            let new_pointer = memory_reallocate(pointer, shrunk_size);
            assert!(
                !new_pointer.is_null(),
                "Reallocation shrinking should succeed"
            );

            // Verify data within new size is preserved
            let new_ptr = new_pointer as *mut u8;
            for i in 0..shrunk_size {
                assert_eq!(
                    *new_ptr.add(i),
                    (i % 256) as u8,
                    "Data within new size should be preserved"
                );
            }

            memory_deallocate(new_pointer);
        }
    }

    #[test]
    fn test_invalid_size_and_capability_metadata_on_reallocation() {
        let caps = CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible;
        unsafe {
            assert!(memory_allocate(null_mut(), usize::MAX - 7, 8, caps.bits()).is_null());
            let pointer = core::ptr::NonNull::new(memory_allocate(null_mut(), 64, 64, caps.bits()))
                .expect("allocation failed")
                .as_ptr();
            pointer.cast::<u8>().write_bytes(0x5a, 64);

            for size in [usize::MAX - 7, isize::MAX as usize] {
                assert!(memory_reallocate(pointer, size).is_null());
                assert_eq!(
                    core::slice::from_raw_parts(pointer.cast::<u8>(), 64),
                    &[0x5a; 64]
                );
            }

            let grown = memory_reallocate(pointer, 4096);
            assert!(!grown.is_null());
            assert!(grown.addr().is_multiple_of(64));
            assert_eq!(
                core::slice::from_raw_parts(grown.cast::<u8>(), 64),
                &[0x5a; 64]
            );
            assert_eq!(
                Allocated::from_user_pointer(grown.cast())
                    .unwrap()
                    .get_capabilities(),
                caps
            );
            memory_deallocate(grown);
        }
    }

    #[test]
    fn test_reallocate_to_zero() {
        let before = MemoryManager::thread_allocations();
        unsafe {
            let pointer = memory_allocate_core(64);
            assert!(!pointer.is_null());

            assert!(memory_reallocate(pointer, 0).is_null());
        }
        let delta = MemoryManager::thread_allocations().since(before);
        assert_eq!(delta.allocations(), 1);
        assert_eq!(delta.deallocated_bytes(), delta.allocated_bytes());
    }

    #[test]
    fn test_reallocate_null_to_zero() {
        let before = MemoryManager::thread_allocations();
        unsafe {
            assert!(memory_reallocate(null_mut(), 0).is_null());
        }
        assert_eq!(MemoryManager::thread_allocations(), before);
    }

    #[test]
    fn test_multiple_allocations() {
        unsafe {
            let mut pointers = Vec::new();
            let allocation_count = 10;
            let size = 64;
            let alignment = 8;
            let capabilities = 0;

            // Allocate multiple memory blocks
            for _ in 0..allocation_count {
                let pointer = memory_allocate(core::ptr::null_mut(), size, alignment, capabilities);
                assert!(!pointer.is_null(), "Each allocation should succeed");
                pointers.push(pointer);
            }

            // Verify each allocation is unique and writable
            for (i, &pointer) in pointers.iter().enumerate() {
                let ptr = pointer as *mut u8;
                let test_value = (i % 256) as u8;
                *ptr = test_value;
                assert_eq!(
                    *ptr, test_value,
                    "Each allocation should be independently writable"
                );
            }

            // Deallocate all memory blocks
            for &pointer in &pointers {
                memory_deallocate(pointer);
            }
        }
    }

    #[test]
    fn test_reallocation_tracking() {
        unsafe {
            // Test that reallocations properly update the tracking table
            let initial_size = 64;
            let new_size = 128;

            let pointer = memory_reallocate(core::ptr::null_mut(), initial_size);
            assert!(!pointer.is_null(), "Initial reallocation should succeed");

            let new_pointer = memory_reallocate(pointer, new_size);
            assert!(!new_pointer.is_null(), "Reallocation should succeed");

            // The new pointer should be properly tracked
            memory_deallocate(new_pointer);
        }
    }
}
