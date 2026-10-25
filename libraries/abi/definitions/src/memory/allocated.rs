use core::{alloc::Layout, ptr::NonNull};

use alloc::slice;

use crate::CompactLayout;

pub struct Allocated {
    pointer: NonNull<u8>,
    padding: u8,
}

impl Allocated {
    const HEADER_SIZE: usize = size_of::<CompactLayout>();

    pub const fn get_padding(alignment: usize) -> usize {
        let misalignment = Self::HEADER_SIZE % alignment;
        if misalignment == 0 {
            0
        } else {
            alignment - misalignment
        }
    }

    pub fn get_layout_for_allocation(size: usize, alignment: usize) -> Layout {
        // Calculate padding needed to align user data after header
        let padding = Self::get_padding(alignment);

        let offset = Self::HEADER_SIZE + padding;
        let total_size = offset + size;

        // The allocation must be aligned to at least the user data alignment
        // to ensure that base_ptr + offset is properly aligned
        Layout::from_size_align(total_size, alignment).expect("Failed to create allocation layout")
    }

    fn new(pointer: *mut u8, padding: usize) -> Option<Self> {
        Some(Self {
            pointer: NonNull::new(pointer)?,
            padding: padding as u8,
        })
    }

    pub fn from_layout(pointer: *mut u8, layout: &Layout) -> Option<Self> {
        let padding = Self::get_padding(layout.align());
        let allocated = Self::new(pointer, padding)?;

        // Store the layout before the user pointer
        allocated.set_layout(layout);

        Some(allocated)
    }

    /// # Safety
    ///
    /// The provided user pointer must have been obtained from a previous allocation
    /// that stored a CompactLayout before the user pointer.
    pub unsafe fn from_user_pointer(user_pointer: *mut u8) -> Option<Self> {
        if user_pointer.is_null() {
            return None;
        }

        // Read the layout stored before the user pointer
        let layout_ptr = unsafe { user_pointer.sub(size_of::<CompactLayout>()) };
        let layout_bytes = unsafe { slice::from_raw_parts(layout_ptr, size_of::<CompactLayout>()) };
        let layout = CompactLayout::from_le_bytes(layout_bytes.try_into().ok()?)?;

        let padding = Self::get_padding(layout.get_alignment());
        let total_header_size = Self::HEADER_SIZE + padding;
        let base_pointer = unsafe { user_pointer.sub(total_header_size) };

        Some(Self {
            pointer: NonNull::new(base_pointer)?,
            padding: padding as u8,
        })
    }

    pub fn get_layout(&self) -> Option<Layout> {
        let layout_pointer = unsafe { self.pointer.as_ptr().add(self.padding as usize) };

        let bytes = unsafe { slice::from_raw_parts(layout_pointer, size_of::<CompactLayout>()) };

        let layout = CompactLayout::from_le_bytes(
            bytes
                .try_into()
                .expect("Failed to read CompactLayout from pointer"),
        )?;

        layout.into_layout()
    }

    pub fn erase_layout(&self) {
        let layout_pointer = unsafe { self.pointer.as_ptr().add(self.padding as usize) };

        let zero_bytes = [0u8; size_of::<CompactLayout>()];

        unsafe {
            core::ptr::copy_nonoverlapping(
                zero_bytes.as_ptr(),
                layout_pointer,
                size_of::<CompactLayout>(),
            );
        }
    }

    pub fn set_layout(&self, layout: &Layout) {
        let bytes = CompactLayout::from_layout(layout)
            .expect("Failed to convert Layout to CompactLayout")
            .to_le_bytes();

        let layout_pointer = unsafe { self.pointer.as_ptr().add(self.padding as usize) };

        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                layout_pointer,
                size_of::<CompactLayout>(),
            );
        }
    }

    pub fn get_base_pointer(&self) -> *mut u8 {
        self.pointer.as_ptr()
    }

    pub fn get_user_pointer(&self) -> *mut u8 {
        let offset = self.padding as usize + Self::HEADER_SIZE;

        unsafe { self.pointer.as_ptr().add(offset) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;

    #[test]
    fn test_get_layout_for_allocation_basic() {
        let layout = Layout::from_size_align(64, 8).unwrap();
        let alloc_layout = Allocated::get_layout_for_allocation(layout.size(), layout.align());

        let padding = Allocated::get_padding(layout.align());
        let offset = Allocated::HEADER_SIZE + padding;
        assert!(offset >= Allocated::HEADER_SIZE);
        assert!(alloc_layout.size() >= layout.size() + Allocated::HEADER_SIZE);
        assert_eq!(alloc_layout.align(), layout.align());
    }

    #[test]
    fn test_get_layout_for_allocation_alignment() {
        let layout = Layout::from_size_align(32, 16).unwrap();
        let padding = Allocated::get_padding(layout.align());

        // User data should be properly aligned
        let offset = Allocated::HEADER_SIZE + padding;
        assert_eq!(offset % layout.align(), 0);
    }

    #[test]
    fn test_new_with_null_pointer() {
        let result = Allocated::new(core::ptr::null_mut(), 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_new_with_valid_pointer() {
        let mut data = [0u8; 64];
        let result = Allocated::new(data.as_mut_ptr(), 8);
        assert!(result.is_some());
    }

    #[test]
    fn test_get_user_pointer_offset() {
        let mut data = [0u8; 64];
        let base_ptr = data.as_mut_ptr();
        let padding = 16usize;

        let allocated = Allocated::new(base_ptr, padding).unwrap();
        let user_ptr = allocated.get_user_pointer();

        assert_eq!(
            user_ptr as usize,
            base_ptr as usize + padding + Allocated::HEADER_SIZE
        );
    }

    #[test]
    fn test_layout_round_trip() {
        let mut data = [0u8; 64];
        let allocated = Allocated::new(data.as_mut_ptr(), 8).unwrap();

        let original_layout = Layout::from_size_align(128, 16).unwrap();
        allocated.set_layout(&original_layout);
        let retrieved_layout = allocated.get_layout().unwrap();

        assert_eq!(original_layout, retrieved_layout);
    }

    #[test]
    fn test_multiple_alignment_values() {
        for align in [1, 2, 4, 8, 16, 32, 64, 128] {
            let padding = Allocated::get_padding(align);
            let offset = Allocated::HEADER_SIZE + padding;
            assert_eq!(offset % align, 0, "Failed for alignment {}", align);
        }
    }

    #[test]
    fn test_zero_size_layout() {
        let layout = Layout::from_size_align(0, 1).unwrap();
        let alloc_layout = Allocated::get_layout_for_allocation(layout.size(), layout.align());
        let padding = Allocated::get_padding(layout.align());

        let offset = Allocated::HEADER_SIZE + padding;
        assert!(offset >= Allocated::HEADER_SIZE);
        assert_eq!(alloc_layout.size(), offset);
    }

    #[test]
    fn test_from_user_pointer_round_trip() {
        // Test that we can recover the Allocated struct from a user pointer
        let mut data = [0u8; 128];
        let base_ptr = data.as_mut_ptr();

        let layout = Layout::from_size_align(64, 16).unwrap();
        let allocated = Allocated::from_layout(base_ptr, &layout).unwrap();

        let user_ptr = allocated.get_user_pointer();
        let recovered = unsafe { Allocated::from_user_pointer(user_ptr).unwrap() };

        // Verify the recovered struct matches the original
        assert_eq!(recovered.pointer, allocated.pointer);
        assert_eq!(recovered.padding, allocated.padding);
        assert_eq!(recovered.get_layout().unwrap(), layout);
    }

    #[test]
    fn test_user_pointer_alignment() {
        // Verify that user pointers are correctly aligned when base pointer is aligned
        use alloc::alloc::{alloc, dealloc};

        for align in [1, 2, 4, 8, 16, 32, 64, 128] {
            let layout = Layout::from_size_align(256, align).unwrap();
            let base_ptr = unsafe { alloc(layout) };
            assert!(!base_ptr.is_null(), "Allocation failed");

            // Now test with the properly aligned base pointer
            let user_layout = Layout::from_size_align(32, align).unwrap();
            let padding = Allocated::get_padding(user_layout.align());

            let allocated = Allocated::new(base_ptr, padding).unwrap();
            let user_ptr = allocated.get_user_pointer();

            // User pointer should be aligned to the requested alignment
            assert_eq!(
                user_ptr as usize % align,
                0,
                "User pointer not aligned to {} bytes",
                align
            );

            unsafe { dealloc(base_ptr, layout) };
        }
    }

    #[test]
    fn test_layout_stored_before_user_pointer() {
        // Verify that the layout is stored immediately before the user pointer
        let mut data = [0u8; 128];
        let base_ptr = data.as_mut_ptr();

        let layout = Layout::from_size_align(64, 8).unwrap();
        let allocated = Allocated::from_layout(base_ptr, &layout).unwrap();
        let user_ptr = allocated.get_user_pointer();

        // Read layout directly from memory just before user pointer
        let layout_ptr = unsafe { user_ptr.sub(Allocated::HEADER_SIZE) };
        let layout_bytes =
            unsafe { core::slice::from_raw_parts(layout_ptr, size_of::<CompactLayout>()) };
        let read_layout = CompactLayout::from_le_bytes(layout_bytes.try_into().unwrap());
        let compact_layout = CompactLayout::from_layout(&layout).unwrap();

        assert_eq!(read_layout.unwrap(), compact_layout);
    }

    #[test]
    fn test_padding_calculation_correctness() {
        // Verify padding calculation ensures proper alignment
        for align in [1, 2, 4, 8, 16, 32, 64, 128] {
            let padding = Allocated::get_padding(align);
            let user_data_offset = Allocated::HEADER_SIZE + padding;

            assert_eq!(
                user_data_offset % align,
                0,
                "Padding calculation failed for alignment {}",
                align
            );
        }
    }

    #[test]
    fn test_allocation_layout_size() {
        // Verify that allocation layout has enough space for header + padding + data
        let layout = Layout::from_size_align(100, 32).unwrap();
        let alloc_layout = Allocated::get_layout_for_allocation(layout.size(), layout.align());
        let padding = Allocated::get_padding(layout.align());

        let offset = Allocated::HEADER_SIZE + padding;
        assert!(alloc_layout.size() >= Allocated::HEADER_SIZE + layout.size());
        assert_eq!(alloc_layout.size(), offset + layout.size());
    }
}
