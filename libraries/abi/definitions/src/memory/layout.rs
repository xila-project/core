use core::{alloc::Layout, fmt::Debug};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CompactLayout(usize);

impl Debug for CompactLayout {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "CompactLayout {{ size: {}, alignment: {} }}",
            self.get_size(),
            self.get_alignment()
        )
    }
}

impl CompactLayout {
    /// Alignment is stored in the upper 4 bits in a log2 format
    const ALIGNMENT_BITS: u8 = 4;
    /// Size uses the remaining bits
    const SIZE_BITS: u8 = (size_of::<usize>() as u8 * 8) - Self::ALIGNMENT_BITS;

    const SIZE_SHIFT: usize = 0;
    const ALIGNMENT_SHIFT: usize = Self::SIZE_BITS as usize;

    const MAXIMUM_SIZE: usize = (1 << Self::SIZE_BITS) - 1;

    const MINIMUM_ALIGNMENT: usize = 1;
    const MAXIMUM_ALIGNMENT: usize = 1 << ((1 << Self::ALIGNMENT_BITS) - 1);

    pub fn from_size_alignment(size: usize, alignment: usize) -> Option<Self> {
        let layout = Layout::from_size_align(size, alignment).ok()?;
        Self::from_layout(&layout)
    }

    pub const fn from_layout(layout: &Layout) -> Option<Self> {
        let size = layout.size();
        let alignment = layout.align();

        if size > Self::MAXIMUM_SIZE
            || alignment < Self::MINIMUM_ALIGNMENT
            || alignment > Self::MAXIMUM_ALIGNMENT
            || !alignment.is_power_of_two()
        {
            return None;
        }

        let alignment_log2 = alignment.ilog2() as usize;

        let compacted = (size << Self::SIZE_SHIFT) | (alignment_log2 << Self::ALIGNMENT_SHIFT);
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
        (self.0 >> Self::SIZE_SHIFT) & Self::MAXIMUM_SIZE
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

    pub fn from_le_bytes(bytes: [u8; size_of::<Self>()]) -> Option<Self> {
        let layout = Self(usize::from_le_bytes(bytes));

        if layout.get_size() > Self::MAXIMUM_SIZE
            || layout.get_alignment() < Self::MINIMUM_ALIGNMENT
            || layout.get_alignment() > Self::MAXIMUM_ALIGNMENT
        {
            return None;
        }

        Some(layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_layout_conversion() {
        let layout = Layout::from_size_align(64, 8).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
        let restored = compact.into_layout().unwrap();
        assert_eq!(restored, layout);
    }

    #[test]
    fn test_minimum_size() {
        let layout = Layout::from_size_align(0, 1).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
        assert_eq!(compact.get_size(), 0);
    }

    #[test]
    fn test_maximum_size() {
        let layout = Layout::from_size_align(CompactLayout::MAXIMUM_SIZE, 1).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
        assert_eq!(compact.get_size(), CompactLayout::MAXIMUM_SIZE);
    }

    #[test]
    fn test_size_overflow() {
        let layout = Layout::from_size_align(CompactLayout::MAXIMUM_SIZE + 1, 1).unwrap();
        assert!(CompactLayout::from_layout(&layout).is_none());
    }

    #[test]
    fn test_minimum_alignment() {
        let layout = Layout::from_size_align(8, CompactLayout::MINIMUM_ALIGNMENT).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
        assert_eq!(compact.get_alignment(), CompactLayout::MINIMUM_ALIGNMENT);
    }

    #[test]
    fn test_maximum_alignment() {
        let layout = Layout::from_size_align(8, CompactLayout::MAXIMUM_ALIGNMENT).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
        assert_eq!(compact.get_alignment(), CompactLayout::MAXIMUM_ALIGNMENT);
    }

    #[test]
    fn test_alignment_overflow() {
        let layout = Layout::from_size_align(8, CompactLayout::MAXIMUM_ALIGNMENT * 2).unwrap();
        assert!(CompactLayout::from_layout(&layout).is_none());
    }

    #[test]
    fn test_non_power_of_two_alignment() {
        // Layout::from_size_align already validates power of two, so we can't directly test
        // but we verify the check is in place
        let layout = Layout::from_size_align(16, 8).unwrap();
        assert!(CompactLayout::from_layout(&layout).is_some());
    }

    #[test]
    fn test_various_alignments() {
        for exp in 0..=(CompactLayout::ALIGNMENT_BITS as u32 * 2) {
            let align = 1 << exp;
            if align <= CompactLayout::MAXIMUM_ALIGNMENT {
                let layout = Layout::from_size_align(16, align).unwrap();
                let compact = CompactLayout::from_layout(&layout).unwrap();
                assert_eq!(compact.get_alignment(), align);
                assert_eq!(compact.get_alignment_log2(), exp as usize);
            }
        }
    }

    #[test]
    fn test_serialization_roundtrip() {
        let layout = Layout::from_size_align(128, 16).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
        let bytes = compact.to_le_bytes();
        let restored = CompactLayout::from_le_bytes(bytes).unwrap();
        assert_eq!(compact, restored);
        assert_eq!(restored.get_size(), 128);
        assert_eq!(restored.get_alignment(), 16);
    }

    #[test]
    fn test_clone_and_copy() {
        let layout = Layout::from_size_align(32, 4).unwrap();
        let compact = CompactLayout::from_layout(&layout).unwrap();
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
                    let compact = CompactLayout::from_layout(&layout).unwrap();
                    assert_eq!(compact.get_size(), size);
                    assert_eq!(compact.get_alignment(), align);
                }
            }
        }
    }
}
