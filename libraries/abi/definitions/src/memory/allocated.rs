use core::{alloc::Layout, ptr::NonNull};

use memory::CapabilityFlags;

use crate::CompactLayout;

pub struct Allocated {
    pointer: NonNull<u8>,
    metadata: CompactLayout,
}

impl Allocated {
    const HEADER_SIZE: usize = size_of::<CompactLayout>();

    const fn user_offset(alignment: usize) -> usize {
        // The header size and the validated alignment are both powers of two.
        if alignment > Self::HEADER_SIZE {
            alignment
        } else {
            Self::HEADER_SIZE
        }
    }

    pub fn get_layout_for_allocation(size: usize, alignment: usize) -> Option<Layout> {
        if !alignment.is_power_of_two() || alignment > CompactLayout::MAXIMUM_ALIGNMENT {
            return None;
        }
        let total_size = Self::user_offset(alignment).checked_add(size)?;
        if total_size > CompactLayout::MAXIMUM_SIZE {
            return None;
        }
        Layout::from_size_align(total_size, alignment).ok()
    }

    /// # Safety
    ///
    /// A non-null `pointer` must refer to a writable allocation with the given
    /// `layout`, including room for the allocation metadata.
    pub unsafe fn from_layout(
        pointer: *mut u8,
        layout: &Layout,
        capabilities: CapabilityFlags,
    ) -> Option<Self> {
        let pointer = NonNull::new(pointer)?;
        let metadata = CompactLayout::from_layout(layout, capabilities)?;
        let allocated = Self { pointer, metadata };

        let header_pointer = unsafe { allocated.get_user_pointer().sub(Self::HEADER_SIZE) };
        unsafe {
            header_pointer
                .cast::<[u8; Self::HEADER_SIZE]>()
                .write(metadata.to_le_bytes())
        };
        Some(allocated)
    }

    /// # Safety
    ///
    /// A non-null `user_pointer` must have been returned by `get_user_pointer` for
    /// a live allocation with intact metadata.
    pub unsafe fn from_user_pointer(user_pointer: *mut u8) -> Option<Self> {
        NonNull::new(user_pointer)?;
        let header_pointer = unsafe { user_pointer.sub(Self::HEADER_SIZE) };
        let bytes = unsafe { header_pointer.cast::<[u8; Self::HEADER_SIZE]>().read() };
        let metadata = CompactLayout::from_le_bytes(bytes);
        let pointer = unsafe {
            NonNull::new_unchecked(user_pointer.sub(Self::user_offset(metadata.get_alignment())))
        };
        Some(Self { pointer, metadata })
    }

    pub fn get_layout(&self) -> Option<Layout> {
        self.metadata.into_layout()
    }

    pub fn get_capabilities(&self) -> CapabilityFlags {
        self.metadata.get_capabilities()
    }

    pub fn get_base_pointer(&self) -> *mut u8 {
        self.pointer.as_ptr()
    }

    pub fn get_user_pointer(&self) -> *mut u8 {
        unsafe {
            self.pointer
                .as_ptr()
                .add(Self::user_offset(self.metadata.get_alignment()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocation_layout() {
        for alignment in [1, 2, 4, 8, 16, 32, 64, 128, 32768] {
            for size in [0, 100] {
                let layout = Allocated::get_layout_for_allocation(size, alignment).unwrap();
                let offset = layout.size() - size;
                assert!(offset >= Allocated::HEADER_SIZE);
                assert_eq!(offset % alignment, 0);
                assert_eq!(layout.align(), alignment);
            }
        }
    }

    #[test]
    fn test_invalid_allocation_layout() {
        for size in [
            usize::MAX - 7,
            isize::MAX as usize,
            CompactLayout::MAXIMUM_SIZE,
        ] {
            assert!(Allocated::get_layout_for_allocation(size, 8).is_none());
        }
        for alignment in [0, 3, CompactLayout::MAXIMUM_ALIGNMENT * 2] {
            assert!(Allocated::get_layout_for_allocation(128, alignment).is_none());
        }
    }

    #[test]
    fn test_metadata_round_trip() {
        use alloc::alloc::{alloc, dealloc};

        for alignment in [1, 2, 4, 8, 16, 32, 64, 128, 32768] {
            let layout = Allocated::get_layout_for_allocation(32, alignment).unwrap();
            let caps = CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible;
            unsafe {
                let base = alloc(layout);
                let allocated = Allocated::from_layout(base, &layout, caps).unwrap();
                let user = allocated.get_user_pointer();
                assert_eq!(user.addr() % alignment, 0);
                assert_eq!(user.offset_from(base) as usize, layout.size() - 32);
                let recovered = Allocated::from_user_pointer(user).unwrap();
                assert_eq!(recovered.get_base_pointer(), base);
                assert_eq!(recovered.get_user_pointer(), user);
                assert_eq!(recovered.get_layout(), Some(layout));
                assert_eq!(recovered.get_capabilities(), caps);
                dealloc(base, layout);
            }
        }
    }
}
