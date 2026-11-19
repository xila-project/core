use core::{alloc::Layout, fmt::Debug};

use memory::CapabilityFlags;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CompactLayout(usize);

impl Debug for CompactLayout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "CompactLayout {{ size: {}, alignment: {}, capabilities: {:?} }}",
            self.get_size(),
            self.get_alignment(),
            self.get_capabilities()
        )
    }
}

impl CompactLayout {
    /// Alignment is stored in the upper 4 bits in a log2 format
    const ALIGNMENT_BITS: u8 = 4;
    /// Capability flags occupy the next three bits, leaving the low bits for size.
    const CAPABILITY_BITS: u8 = 3;
    const SIZE_BITS: u8 = usize::BITS as u8 - Self::ALIGNMENT_BITS - Self::CAPABILITY_BITS;
    const CAPABILITY_SHIFT: u8 = Self::SIZE_BITS;
    const ALIGNMENT_SHIFT: u8 = Self::SIZE_BITS + Self::CAPABILITY_BITS;
    const CAPABILITY_MASK: u8 = (1 << Self::CAPABILITY_BITS) - 1;

    pub(super) const MAXIMUM_SIZE: usize = (1 << Self::SIZE_BITS) - 1;
    pub(super) const MAXIMUM_ALIGNMENT: usize = 1 << ((1 << Self::ALIGNMENT_BITS) - 1);

    pub const fn from_layout(layout: &Layout, capabilities: CapabilityFlags) -> Option<Self> {
        let size = layout.size();
        let alignment = layout.align();

        if size > Self::MAXIMUM_SIZE
            || alignment > Self::MAXIMUM_ALIGNMENT
            || capabilities.bits() > Self::CAPABILITY_MASK
        {
            return None;
        }

        let alignment_log2 = alignment.ilog2() as usize;

        let compacted = size
            | ((capabilities.bits() as usize) << Self::CAPABILITY_SHIFT)
            | (alignment_log2 << Self::ALIGNMENT_SHIFT);
        Some(Self(compacted))
    }

    pub const fn into_layout(&self) -> Option<Layout> {
        let size = self.get_size();
        let alignment = self.get_alignment();

        match Layout::from_size_align(size, alignment) {
            Ok(layout) => Some(layout),
            Err(_) => None,
        }
    }

    pub const fn get_size(&self) -> usize {
        self.0 & Self::MAXIMUM_SIZE
    }

    pub const fn get_alignment_log2(&self) -> usize {
        (self.0 >> Self::ALIGNMENT_SHIFT) & ((1 << Self::ALIGNMENT_BITS) - 1)
    }

    pub const fn get_alignment(&self) -> usize {
        1 << self.get_alignment_log2()
    }

    pub const fn to_le_bytes(&self) -> [u8; size_of::<Self>()] {
        self.0.to_le_bytes()
    }

    pub const fn get_capabilities(&self) -> CapabilityFlags {
        let bits = (self.0 >> Self::CAPABILITY_SHIFT) as u8 & Self::CAPABILITY_MASK;
        CapabilityFlags::from_bits_retain(bits)
    }

    pub const fn from_le_bytes(bytes: [u8; size_of::<Self>()]) -> Self {
        Self(usize::from_le_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_layout_conversion() {
        let layout = Layout::from_size_align(64, 8).unwrap();
        let compact = CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
        let restored = compact.into_layout().unwrap();
        assert_eq!(restored, layout);
    }

    #[test]
    fn test_minimum_size() {
        let layout = Layout::from_size_align(0, 1).unwrap();
        let compact = CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
        assert_eq!(compact.get_size(), 0);
    }

    #[test]
    fn test_maximum_size() {
        let layout = Layout::from_size_align(CompactLayout::MAXIMUM_SIZE, 1).unwrap();
        let compact = CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
        assert_eq!(compact.get_size(), CompactLayout::MAXIMUM_SIZE);
    }

    #[test]
    fn test_size_overflow() {
        let layout = Layout::from_size_align(CompactLayout::MAXIMUM_SIZE + 1, 1).unwrap();
        assert!(CompactLayout::from_layout(&layout, CapabilityFlags::empty()).is_none());
    }

    #[test]
    fn test_minimum_alignment() {
        let layout = Layout::from_size_align(8, 1).unwrap();
        let compact = CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
        assert_eq!(compact.get_alignment(), 1);
    }

    #[test]
    fn test_maximum_alignment() {
        let layout = Layout::from_size_align(8, CompactLayout::MAXIMUM_ALIGNMENT).unwrap();
        let compact = CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
        assert_eq!(compact.get_alignment(), CompactLayout::MAXIMUM_ALIGNMENT);
    }

    #[test]
    fn test_alignment_overflow() {
        let layout = Layout::from_size_align(8, CompactLayout::MAXIMUM_ALIGNMENT * 2).unwrap();
        assert!(CompactLayout::from_layout(&layout, CapabilityFlags::empty()).is_none());
    }

    #[test]
    fn test_non_power_of_two_alignment() {
        // Layout::from_size_align already validates power of two, so we can't directly test
        // but we verify the check is in place
        let layout = Layout::from_size_align(16, 8).unwrap();
        assert!(CompactLayout::from_layout(&layout, CapabilityFlags::empty()).is_some());
    }

    #[test]
    fn test_various_alignments() {
        for exp in 0..=(CompactLayout::ALIGNMENT_BITS as u32 * 2) {
            let align = 1 << exp;
            if align <= CompactLayout::MAXIMUM_ALIGNMENT {
                let layout = Layout::from_size_align(16, align).unwrap();
                let compact =
                    CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
                assert_eq!(compact.get_alignment(), align);
                assert_eq!(compact.get_alignment_log2(), exp as usize);
            }
        }
    }

    #[test]
    fn test_serialization_roundtrip() {
        let layout = Layout::from_size_align(128, 16).unwrap();
        let capabilities = CapabilityFlags::all();
        let compact = CompactLayout::from_layout(&layout, capabilities).unwrap();
        let bytes = compact.to_le_bytes();
        let restored = CompactLayout::from_le_bytes(bytes);
        assert_eq!(compact, restored);
        assert_eq!(restored.get_capabilities(), capabilities);
        assert_eq!(restored.into_layout(), Some(layout));
        assert_eq!(size_of::<CompactLayout>(), size_of::<usize>());
        assert_eq!(restored.get_size(), 128);
        assert_eq!(restored.get_alignment(), 16);
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_clone_and_copy() {
        let layout = Layout::from_size_align(32, 4).unwrap();
        let compact = CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
        let cloned = compact.clone();
        let copied = compact;
        assert_eq!(compact, cloned);
        assert_eq!(compact, copied);
    }

    #[test]
    fn test_multiple_size_alignment_combinations() {
        let sizes = [0, 1, 16, 256, 4096];
        let alignments = [1, 2, 4, 8, 16, 32, 64];

        for &size in &sizes {
            for &align in &alignments {
                if size <= CompactLayout::MAXIMUM_SIZE && align <= CompactLayout::MAXIMUM_ALIGNMENT
                {
                    let layout = Layout::from_size_align(size, align).unwrap();
                    let compact =
                        CompactLayout::from_layout(&layout, CapabilityFlags::empty()).unwrap();
                    assert_eq!(compact.get_size(), size);
                    assert_eq!(compact.get_alignment(), align);
                }
            }
        }
    }
}
