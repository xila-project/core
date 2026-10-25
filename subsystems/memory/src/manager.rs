use core::{
    alloc::GlobalAlloc,
    ptr::{NonNull, null_mut},
};

use crate::{CapabilityFlags, Layout, ManagerTrait};

unsafe extern "Rust" {
    unsafe static __XILA_MEMORY_MANAGER: Manager<'static>;
}

/// A wrapper type that adapts any type implementing `Allocator_trait` to the standard
/// Rust `GlobalAlloc` trait.
///
/// This enables custom allocators that implement the Xila-specific `Allocator_trait`
/// to be used as the global memory allocator for Rust's allocation system.
pub struct Manager<'a>(&'a dyn ManagerTrait);

impl<'a> Manager<'a> {
    /// Creates a new instance of `Allocator_type` wrapping the provided allocator.
    ///
    /// # Parameters
    /// * `Allocator` - The allocator to wrap
    ///
    /// # Returns
    /// A new instance of `Allocator_type` containing the provided allocator.
    pub const fn new(allocator: &'a dyn ManagerTrait) -> Self {
        Self(allocator)
    }

    /// Allocates memory with the specified capabilities and layout.
    ///
    /// # Parameters
    /// * `Capabilities` - Specific requirements for the allocation
    /// * `Layout` - Size and alignment requirements for the allocation
    ///
    /// # Returns
    /// A pointer to the allocated memory, or a null pointer if allocation failed.
    ///
    /// # Safety
    /// This function is unsafe because the caller must ensure that:
    pub unsafe fn allocate(&self, capabilities: CapabilityFlags, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return null_mut();
        }

        match unsafe { self.0.allocate(capabilities, layout) } {
            Some(ptr) => ptr.as_ptr(),
            None => {
                null_mut()
                //        panic!(
                //            "xila_memory_allocate for capabilities {:?} and layout {:?} failed",
                //            capabilities, layout
                //        );
            }
        }
    }

    /// Deallocates memory previously allocated by this allocator.
    ///
    /// # Parameters
    /// * `Pointer` - Pointer to the memory to deallocate
    /// * `Layout` - The layout that was used to allocate the memory
    ///
    /// # Safety
    /// This function is unsafe because the caller must ensure that:
    /// - The pointer was allocated by this allocator
    /// - The layout matches the layout used for allocation
    pub unsafe fn deallocate(&self, pointer: *mut u8, layout: Layout) {
        if let Some(pointer) = NonNull::new(pointer) {
            unsafe { self.0.deallocate(pointer, layout) }
        } else {
            // No panic since its allowed in C
            // panic!("Attempted to deallocate a null pointer");
        }
    }

    /// Reallocates memory with a new layout.
    ///
    /// This method changes the size or alignment of a previously allocated memory block.
    /// If the pointer is `None`, this behaves like a normal allocation.
    /// If reallocation is successful, the contents of the old memory are preserved
    /// up to the minimum of the old and new sizes.
    ///
    /// # Parameters
    /// * `Pointer` - Pointer to the memory to reallocate. If null, acts as a new allocation
    /// * `Old_layout` - The layout that was used for the original allocation
    /// * `New_size` - The new size for the memory
    ///
    /// # Returns
    /// A pointer to the reallocated memory with the new layout, or a null pointer if reallocation failed.
    ///
    /// # Safety
    /// This function is unsafe because the caller must ensure that:
    /// - If `Pointer` is not null, it was allocated by this allocator
    /// - The `Old_layout` matches the one used for the original allocation
    /// - The old memory is not used after successful reallocation
    /// - The returned memory is properly initialized before use
    pub unsafe fn reallocate(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = Layout::from_size_align(new_size, layout.align()).unwrap_or(layout);

        let pointer = match NonNull::new(pointer) {
            Some(pointer) => unsafe { self.0.reallocate(pointer, layout, new_layout) },
            None => unsafe { self.0.allocate(CapabilityFlags::empty(), new_layout) },
        };

        pointer.map_or(null_mut(), |ptr| ptr.as_ptr())
    }

    pub fn get_page_size(&self) -> usize {
        self.0.get_page_size()
    }

    pub fn flush_data_cache(&self) {
        self.0.flush_data_cache()
    }

    /// Flushes the instruction cache for the specified memory region.
    ///
    /// # Parameters
    /// * `Address` - Starting address of the memory region
    /// * `Size` - Size of the memory region in bytes
    /// # Safety
    /// This function is unsafe because the caller must ensure that:
    /// - The address is valid and points to a memory region of at least `size` bytes
    /// - The memory region is not being modified while the instruction cache is being flushed
    pub unsafe fn flush_instruction_cache(&self, address: *const u8, size: usize) {
        let address = if let Some(address) = NonNull::new(address as *mut u8) {
            address
        } else {
            log::warning!("flush_instruction_cache called with null address, ignoring");
            return;
        };

        self.0.flush_instruction_cache(address, size)
    }

    /// Returns the amount of memory currently used.
    ///
    /// # Returns
    /// The number of bytes currently allocated.
    pub fn get_used(&self) -> usize {
        self.0.get_used()
    }

    /// Returns the amount of memory currently available.
    ///
    /// # Returns
    /// The number of bytes available for allocation.
    ///
    pub fn get_free(&self) -> usize {
        self.0.get_free()
    }

    /// Returns the total size of the memory managed by this allocator.
    ///
    /// # Returns
    /// The total number of bytes managed by the allocator.
    pub fn get_total_size(&self) -> usize {
        self.0.get_total_size()
    }
}

/// Implementation of the standard library's `GlobalAlloc` trait for any wrapped
/// type that implements `Allocator_trait`.
///
/// This implementation delegates the standard allocation operations to the wrapped
/// allocator's methods, converting between Rust's and Xila's allocation types.
///
/// # Safety
/// The implementation upholds the safety guarantees required by `GlobalAlloc`:
/// - Memory is properly aligned according to the layout
/// - Deallocation uses the same layout that was used for allocation
unsafe impl<'a> GlobalAlloc for Manager<'a> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.allocate(CapabilityFlags::empty(), layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { self.deallocate(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { self.reallocate(pointer, layout, new_size) }
    }
}

pub fn global_manager() -> &'static Manager<'static> {
    unsafe { &__XILA_MEMORY_MANAGER }
}
