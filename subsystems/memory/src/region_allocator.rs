//! Portable TLSF storage. Platforms retain ownership of regions and synchronization.
use core::{alloc::Layout, ptr::NonNull};

/// A caller-owned, contiguous heap with bounded TLSF allocation/free lookup.
///
/// `FL` controls maximum block size: `(4 * size_of::<usize>() << FL) - granularity`.
/// Larger pools are split into chunks by rlsf. `SL` must be a power of two <= 32.
/// The default 16 x 8 classes cover MCU heaps while using 580 bytes on 32-bit.
/// Native/Wasm managers select `usize::BITS` levels for contiguous pool growth.
/// No locking is performed here; callers must serialize access.
///
/// Statistics include allocated block headers/padding in `used`, and free block
/// headers in `free`; pool sentinels and unaligned edges are excluded from totals.
/// Statistics walk blocks (O(n)); allocation and free do not walk the heap.
pub struct RegionHeap<const FL: usize = 16, const SL: usize = 8> {
    tlsf: rlsf::Tlsf<'static, usize, u32, FL, SL>,
    start: *mut u8,
    len: usize,
    inserted: usize,
}

// Safety: storage is exclusively owned under init's contract; access requires &mut.
unsafe impl<const FL: usize, const SL: usize> Send for RegionHeap<FL, SL> {}

impl<const FL: usize, const SL: usize> Default for RegionHeap<FL, SL> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<const FL: usize, const SL: usize> RegionHeap<FL, SL> {
    pub const fn empty() -> Self {
        Self {
            tlsf: rlsf::Tlsf::new(),
            start: core::ptr::null_mut(),
            len: 0,
            inserted: 0,
        }
    }

    /// Initialize an empty heap. Small/unaligned pools can have no usable space.
    ///
    /// # Safety
    /// `start..start+size` must be valid writable memory exclusively owned by this
    /// heap until it is dropped, and there must be no outstanding allocations.
    pub unsafe fn init(&mut self, start: *mut u8, size: usize) {
        *self = Self::empty();
        self.start = start;
        self.len = size;
        if let Some(pool) = NonNull::new(core::ptr::slice_from_raw_parts_mut(start, size)) {
            self.inserted = unsafe { self.tlsf.insert_free_block_ptr(pool) }.map_or(0, |n| n.get());
        }
    }

    /// Add contiguous bytes after the original region, preserving allocations.
    ///
    /// # Safety
    /// The additional bytes must be valid, writable, and exclusively owned by this
    /// heap, immediately following the storage provided to `init`/previous `extend`.
    pub unsafe fn extend(&mut self, additional: usize) {
        let new_len = self
            .len
            .checked_add(additional)
            .expect("heap size overflow");
        assert!(!self.start.is_null());
        if self.inserted == 0 {
            unsafe { self.init(self.start, new_len) };
            return;
        }
        // insert_free_block_ptr returns the span from the original start,
        // including leading alignment loss. Preserve any trailing partial bytes.
        let pool = NonNull::slice_from_raw_parts(
            unsafe { NonNull::new_unchecked(self.start.add(self.inserted)) },
            new_len - self.inserted,
        );
        self.inserted += unsafe { self.tlsf.append_free_block_ptr(pool) };
        self.len = new_len;
    }

    /// Conservative additional storage for a request, including size-class search
    /// rounding, alignment, sentinel and unaligned pool edges. Platforms use this
    /// for growth; fixed heaps still return None when a request cannot fit.
    pub fn growth_size(layout: Layout) -> Option<usize> {
        let granularity = rlsf::GRANULARITY;
        let search = layout
            .size()
            .checked_add(layout.align())?
            .checked_add(granularity)?;
        search
            .checked_add(search / SL)?
            .checked_add(3 * granularity)
    }

    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if layout.size() == 0 {
            return None;
        }
        self.tlsf.allocate(layout)
    }

    /// # Safety
    /// `pointer` must be live in this heap with the given allocation layout.
    pub unsafe fn deallocate(&mut self, pointer: NonNull<u8>, layout: Layout) {
        unsafe { self.tlsf.deallocate(pointer, layout.align()) };
    }

    /// Reallocate within the same region, preserving bytes on success and the old
    /// allocation on failure. Moving reallocation copies O(min(old,new)) bytes.
    ///
    /// # Safety
    /// `pointer` must be live in this heap with layout `old`.
    pub unsafe fn reallocate(
        &mut self,
        pointer: NonNull<u8>,
        old: Layout,
        new: Layout,
    ) -> Option<NonNull<u8>> {
        if new.size() == 0 {
            return None;
        }
        if old.align() == new.align() {
            return unsafe { self.tlsf.reallocate(pointer, new) };
        }
        let result = self.allocate(new)?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                pointer.as_ptr(),
                result.as_ptr(),
                old.size().min(new.size()),
            );
            self.deallocate(pointer, old);
        }
        Some(result)
    }

    pub fn statistics(&self) -> crate::RegionStatistics {
        let mut stats = crate::RegionStatistics {
            size: 0,
            used: 0,
            free: 0,
        };
        if self.inserted == 0 {
            return stats;
        }
        let pool = NonNull::slice_from_raw_parts(
            unsafe { NonNull::new_unchecked(self.start) },
            self.inserted,
        );
        // Safety: exactly the span accepted by insert/append, still exclusively owned.
        for block in unsafe { self.tlsf.iter_blocks(pool) } {
            stats.size += block.size();
            if block.is_occupied() {
                stats.used += block.size();
            } else {
                stats.free += block.size();
            }
        }
        stats
    }

    pub fn used(&self) -> usize {
        self.statistics().used
    }
    pub fn free(&self) -> usize {
        self.statistics().free
    }
}
