use core::{
    cell::RefCell,
    mem::MaybeUninit,
    ptr::{NonNull, copy_nonoverlapping},
};

use critical_section::Mutex;

use crate::{
    CapabilityFlags, Layout, ManagerTrait, RegionStatistics, Statistics,
    heap::{Heap, Resize},
};

/// Configuration metadata for a single static memory region managed by [`RegionManager`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionDescriptor {
    /// Capabilities provided by allocations placed in this region.
    capabilities: CapabilityFlags,
    /// Minimum alignment (for both pointer address and allocation size) required when serving
    /// [`CapabilityFlags::DirectMemoryAccess`] allocations from this region.
    ///
    /// A value of `1` imposes no extra DMA cache-line alignment constraint beyond the caller's
    /// [`Layout`].
    dma_alignment_shift: u8,
}

impl RegionDescriptor {
    /// Creates a region descriptor with the given capability flags and no extra DMA alignment
    /// constraint (`dma_alignment = 1`).
    pub const fn new(capabilities: CapabilityFlags) -> Self {
        Self {
            capabilities,
            dma_alignment_shift: 0,
        }
    }

    /// Sets the minimum address and size alignment required when serving
    /// [`CapabilityFlags::DirectMemoryAccess`] allocations from this region.
    ///
    /// # Panics
    ///
    /// Panics if `dma_alignment` is not a power of two.
    pub const fn with_dma_alignment(mut self, dma_alignment: usize) -> Self {
        assert!(
            dma_alignment.is_power_of_two(),
            "DMA alignment must be a power of two"
        );
        self.dma_alignment_shift = dma_alignment.trailing_zeros() as u8;
        self
    }
}

struct Region {
    heap: Heap,
    descriptor: RegionDescriptor,
}

impl Region {
    const fn new(descriptor: RegionDescriptor) -> Self {
        Self {
            heap: Heap::empty(),
            descriptor,
        }
    }

    fn is_empty(&self) -> bool {
        self.heap.size() == 0
    }

    fn dma_alignment(&self) -> usize {
        1usize << self.descriptor.dma_alignment_shift
    }

    fn supports(&self, capabilities: CapabilityFlags, layout: Layout) -> bool {
        if self.is_empty() || !self.descriptor.capabilities.contains(capabilities) {
            return false;
        }

        if capabilities.contains(CapabilityFlags::DirectMemoryAccess) {
            let alignment = self.dma_alignment();
            return layout.align() >= alignment && layout.size().is_multiple_of(alignment);
        }

        true
    }

    /// Alignment a block moved out of `pointer` must keep so that it still satisfies this region's
    /// DMA constraint.
    ///
    /// A block carries no record of the capabilities it was allocated with. In a DMA-capable region,
    /// a block placed at a DMA-aligned address is treated as a DMA block and keeps that alignment;
    /// the size rule stays with the caller, as it does for any DMA allocation.
    fn preserved_alignment(&self, pointer: NonNull<u8>) -> usize {
        let alignment = self.dma_alignment();
        if self
            .descriptor
            .capabilities
            .contains(CapabilityFlags::DirectMemoryAccess)
            && pointer.as_ptr().addr().is_multiple_of(alignment)
        {
            alignment
        } else {
            1
        }
    }

    fn contains(&self, pointer: NonNull<u8>, layout: Layout) -> bool {
        if self.is_empty() {
            return false;
        }

        let start = self.heap.bottom() as usize;
        let end = self.heap.top() as usize;
        let ptr = pointer.as_ptr() as usize;

        ptr >= start
            && ptr
                .checked_add(layout.size())
                .is_some_and(|ptr_end| ptr_end <= end)
    }

    fn statistics(&self) -> Option<RegionStatistics> {
        if self.is_empty() {
            return None;
        }

        Some(RegionStatistics {
            size: self.heap.size(),
            used: self.heap.used(),
            free: self.heap.free(),
        })
    }
}

#[cfg(feature = "Debug")]
struct InternalStats {
    max_usage: usize,
    total_allocated: usize,
    total_freed: usize,
}

#[cfg(feature = "Debug")]
impl InternalStats {
    const fn new() -> Self {
        Self {
            max_usage: 0,
            total_allocated: 0,
            total_freed: 0,
        }
    }
}

struct RegionManagerInner<const N: usize> {
    regions: [Region; N],
    large_allocation_threshold: usize,
    #[cfg(feature = "Debug")]
    stats: InternalStats,
}

impl<const N: usize> RegionManagerInner<N> {
    fn total_used(&self) -> usize {
        self.regions.iter().map(|region| region.heap.used()).sum()
    }

    fn allocate_from_region(
        region: &mut Region,
        capabilities: CapabilityFlags,
        layout: Layout,
    ) -> Option<NonNull<u8>> {
        if !region.supports(capabilities, layout) {
            return None;
        }
        region.heap.allocate(layout)
    }

    /// Index of the region that owns the live block `pointer` of `layout`.
    ///
    /// # Panics
    ///
    /// Panics if no region owns the block.
    fn region_index(&self, pointer: NonNull<u8>, layout: Layout) -> usize {
        self.regions
            .iter()
            .position(|region| region.contains(pointer, layout))
            .expect("Pointer does not belong to any managed memory region")
    }

    #[cfg(feature = "Debug")]
    fn record_usage_change(&mut self, used_before: usize) {
        let used_after = self.total_used();
        if used_after >= used_before {
            let delta = used_after - used_before;
            self.stats.total_allocated = self.stats.total_allocated.saturating_add(delta);
            if used_after > self.stats.max_usage {
                self.stats.max_usage = used_after;
            }
        } else {
            let delta = used_before - used_after;
            self.stats.total_freed = self.stats.total_freed.saturating_add(delta);
        }
    }

    fn allocate(&mut self, capabilities: CapabilityFlags, layout: Layout) -> Option<NonNull<u8>> {
        if layout.size() == 0 {
            return None;
        }

        #[cfg(feature = "Debug")]
        let used_before = self.total_used();

        let prefer_reverse = layout.size() >= self.large_allocation_threshold;

        let allocation = if prefer_reverse {
            self.regions
                .iter_mut()
                .rev()
                .find_map(|region| Self::allocate_from_region(region, capabilities, layout))
        } else {
            self.regions
                .iter_mut()
                .find_map(|region| Self::allocate_from_region(region, capabilities, layout))
        };

        #[cfg(feature = "Debug")]
        if allocation.is_some() {
            let used_after = self.total_used();
            let delta = used_after.saturating_sub(used_before);
            self.stats.total_allocated = self.stats.total_allocated.saturating_add(delta);
            if used_after > self.stats.max_usage {
                self.stats.max_usage = used_after;
            }
        }

        allocation
    }

    unsafe fn deallocate(&mut self, pointer: NonNull<u8>, layout: Layout) {
        if layout.size() == 0 {
            return;
        }

        #[cfg(feature = "Debug")]
        let used_before = self.total_used();

        let index = self.region_index(pointer, layout);

        unsafe { self.regions[index].heap.deallocate(pointer, layout) };

        #[cfg(feature = "Debug")]
        {
            let used_after = self.total_used();
            let delta = used_before.saturating_sub(used_after);
            self.stats.total_freed = self.stats.total_freed.saturating_add(delta);
        }
    }
}

impl<const N: usize> RegionManagerInner<N> {
    /// Resizes a live block without moving it.
    ///
    /// The block stays in its region, so it keeps every capability the region gave it.
    ///
    /// # Safety
    ///
    /// `pointer` must be a live block of `old_layout` from this manager.
    unsafe fn resize_in_place(
        &mut self,
        pointer: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Resize {
        if !pointer.as_ptr().addr().is_multiple_of(new_layout.align()) {
            return Resize::Impossible;
        }

        #[cfg(feature = "Debug")]
        let used_before = self.total_used();

        let index = self.region_index(pointer, old_layout);
        let resize = unsafe {
            self.regions[index]
                .heap
                .resize_in_place(pointer, old_layout, new_layout)
        };

        #[cfg(feature = "Debug")]
        if resize == Resize::Done {
            self.record_usage_change(used_before);
        }

        resize
    }

    /// Allocates the block a reallocation moves to.
    ///
    /// The region of the old block is tried first, so that a block keeps the capabilities of its
    /// region, then the ordinary placement order applies.
    ///
    /// # Safety
    ///
    /// If `old_layout` is not empty, `old_pointer` must be a live block of it from this manager.
    unsafe fn allocate_moved(
        &mut self,
        old_pointer: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Option<NonNull<u8>> {
        if old_layout.size() > 0 {
            let index = self.region_index(old_pointer, old_layout);
            let region = &mut self.regions[index];
            let alignment = new_layout
                .align()
                .max(region.preserved_alignment(old_pointer));

            if let Ok(layout) = Layout::from_size_align(new_layout.size(), alignment) {
                #[cfg(feature = "Debug")]
                let used_before = self.total_used();

                if let Some(pointer) = self.regions[index].heap.allocate(layout) {
                    #[cfg(feature = "Debug")]
                    self.record_usage_change(used_before);
                    return Some(pointer);
                }
            }
        }

        self.allocate(CapabilityFlags::empty(), new_layout)
    }
}

fn is_valid_heap_range(start: *mut u8, size: usize) -> bool {
    if start.is_null() {
        return false;
    }

    let align = core::mem::align_of::<usize>();
    let min_hole = 2 * core::mem::size_of::<usize>();
    let addr = start as usize;

    let Some(aligned_start) = addr.checked_add(align - 1).map(|v| v & !(align - 1)) else {
        return false;
    };
    let Some(end) = addr.checked_add(size) else {
        return false;
    };
    let aligned_end = end & !(align - 1);

    aligned_end.saturating_sub(aligned_start) >= min_hole
}

/// Static multi-region memory manager backed by `linked_list_allocator`.
///
/// Manages a fixed compile-time count `N` of non-overlapping memory regions without heap-allocated
/// bookkeeping or per-allocation headers. Regions are ordered from highest-priority / fastest
/// (`0`) to lowest-priority / largest (`N - 1`).
///
/// When `large_allocation_threshold` is `Some(threshold)`, allocations smaller than `threshold`
/// probe regions in forward order (`0..N`) and allocations `>= threshold` probe in reverse order
/// (`(0..N).rev()`), falling back across the remaining capable regions if the preferred region
/// is uninitialized, lacks a requested capability, violates DMA alignment, or is full.
///
/// Allocation and deallocation can access free lists in any managed region. Perform
/// those operations while all registered regions are accessible.
pub struct RegionManager<const N: usize> {
    inner: Mutex<RefCell<RegionManagerInner<N>>>,
}

impl<const N: usize> RegionManager<N> {
    /// Creates a new uninitialized multi-region manager from static descriptors.
    pub const fn new(
        descriptors: [RegionDescriptor; N],
        large_allocation_threshold: Option<usize>,
    ) -> Self {
        let mut regions =
            [const { Region::new(RegionDescriptor::new(CapabilityFlags::empty())) }; N];
        let mut i = 0;
        while i < N {
            regions[i] = Region::new(descriptors[i]);
            i += 1;
        }

        let threshold = match large_allocation_threshold {
            Some(threshold) => threshold,
            None => usize::MAX,
        };

        Self {
            inner: Mutex::new(RefCell::new(RegionManagerInner {
                regions,
                large_allocation_threshold: threshold,
                #[cfg(feature = "Debug")]
                stats: InternalStats::new(),
            })),
        }
    }

    /// Initializes region `index` with `[start, start + size)`.
    ///
    /// Returns `false` if `start` is null or `size` is too small for allocator metadata,
    /// leaving the region uninitialized.
    ///
    /// # Panics
    ///
    /// Panics if `index >= N` or if region `index` has already been initialized.
    ///
    /// # Safety
    ///
    /// For a non-null start, the caller must ensure that `[start, start + size)` is valid, unaliased, writable memory
    /// for `'static`, and does not overlap any other region managed by this instance.
    pub unsafe fn init_region(&self, index: usize, start: *mut u8, size: usize) -> bool {
        if !is_valid_heap_range(start, size) {
            return false;
        }

        critical_section::with(|cs| {
            let mut inner = self.inner.borrow_ref_mut(cs);
            let region = &mut inner.regions[index];
            assert!(region.is_empty(), "memory region already initialized");
            unsafe { region.heap.init(start, size) };
        });

        true
    }

    /// Initializes region `index` from a static mutable slice.
    ///
    /// # Panics
    ///
    /// Panics if `index >= N` or if region `index` has already been initialized.
    pub fn init_region_from_slice(
        &self,
        index: usize,
        slice: &'static mut [MaybeUninit<u8>],
    ) -> bool {
        let size = slice.len();
        let start = slice.as_mut_ptr().cast::<u8>();
        unsafe { self.init_region(index, start, size) }
    }

    /// Returns whether region `index` has been initialized.
    pub fn is_region_initialized(&self, index: usize) -> bool {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            inner
                .regions
                .get(index)
                .is_some_and(|region| !region.is_empty())
        })
    }

    /// Returns whether `pointer` falls within initialized region `index`'s address range.
    pub fn region_contains(&self, index: usize, pointer: NonNull<u8>) -> bool {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            let Some(region) = inner.regions.get(index) else {
                return false;
            };
            if region.is_empty() {
                return false;
            }
            let addr = pointer.as_ptr() as usize;
            let start = region.heap.bottom() as usize;
            let end = region.heap.top() as usize;
            addr >= start && addr < end
        })
    }

    /// Returns total free bytes across initialized regions advertising `capabilities`.
    ///
    /// This does not account for layout constraints or fragmentation.
    pub fn get_free_for_capabilities(&self, capabilities: CapabilityFlags) -> usize {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            inner
                .regions
                .iter()
                .filter(|region| {
                    !region.is_empty() && region.descriptor.capabilities.contains(capabilities)
                })
                .map(|region| region.heap.free())
                .sum()
        })
    }

    /// Returns per-region and aggregate heap statistics.
    pub fn get_statistics(&self) -> Statistics<N> {
        critical_section::with(|cs| {
            let inner = self.inner.borrow_ref(cs);
            let mut region_stats = [const { None }; N];
            let mut size = 0usize;
            let mut current_usage = 0usize;

            for (slot, region) in region_stats.iter_mut().zip(inner.regions.iter()) {
                if let Some(stats) = region.statistics() {
                    size += stats.size;
                    current_usage += stats.used;
                    *slot = Some(stats);
                }
            }

            Statistics {
                region_stats,
                size,
                current_usage,
                #[cfg(feature = "Debug")]
                max_usage: inner.stats.max_usage,
                #[cfg(feature = "Debug")]
                total_allocated: inner.stats.total_allocated,
                #[cfg(feature = "Debug")]
                total_freed: inner.stats.total_freed,
            }
        })
    }
}

impl<const N: usize> ManagerTrait for RegionManager<N> {
    unsafe fn allocate(
        &self,
        capabilities: CapabilityFlags,
        layout: Layout,
    ) -> Option<NonNull<u8>> {
        critical_section::with(|cs| self.inner.borrow_ref_mut(cs).allocate(capabilities, layout))
    }

    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        critical_section::with(|cs| unsafe {
            self.inner.borrow_ref_mut(cs).deallocate(pointer, layout);
        });
    }

    /// Resizes a block, in place when possible.
    ///
    /// * A shrink keeps the pointer; the freed tail returns to the region's free list. The one
    ///   exception is a shrink by a single word with no free memory after the block: the region
    ///   cannot record such a tail, so the block moves to stay exact, or, if nothing can hold the
    ///   smaller block, stays as it is with that word as slack.
    /// * A growth keeps the pointer when the memory right after the block is free.
    /// * Otherwise the block moves: the region of the old block is tried first, then the ordinary
    ///   placement order applies. The copy runs outside the critical section.
    ///
    /// A block never leaves its region while it can be resized in place or fits elsewhere in that
    /// region, so it keeps the capabilities and DMA alignment of the region it was allocated in.
    /// Regions are only left when the old region is full.
    unsafe fn reallocate(
        &self,
        old_pointer: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Option<NonNull<u8>> {
        let old_size = old_layout.size();
        let new_size = new_layout.size();

        if new_size == 0 {
            return None;
        }

        let mut resize = Resize::Impossible;
        if old_size > 0 {
            if old_layout == new_layout {
                return Some(old_pointer);
            }

            resize = critical_section::with(|cs| unsafe {
                self.inner
                    .borrow_ref_mut(cs)
                    .resize_in_place(old_pointer, old_layout, new_layout)
            });
            if resize == Resize::Done {
                return Some(old_pointer);
            }
        }

        let Some(new_pointer) = critical_section::with(|cs| unsafe {
            self.inner
                .borrow_ref_mut(cs)
                .allocate_moved(old_pointer, old_layout, new_layout)
        }) else {
            // A shrink must not fail: keep the block, and the word the region cannot take back.
            return (resize == Resize::OneWordTail).then_some(old_pointer);
        };

        if old_size > 0 {
            let copy_size = core::cmp::min(old_size, new_size);
            unsafe {
                copy_nonoverlapping(old_pointer.as_ptr(), new_pointer.as_ptr(), copy_size);
            }
            unsafe { self.deallocate(old_pointer, old_layout) };
        }

        Some(new_pointer)
    }

    fn get_used(&self) -> usize {
        critical_section::with(|cs| self.inner.borrow_ref(cs).total_used())
    }

    fn get_free(&self) -> usize {
        critical_section::with(|cs| {
            self.inner
                .borrow_ref(cs)
                .regions
                .iter()
                .map(|region| region.heap.free())
                .sum()
        })
    }

    fn get_total_size(&self) -> usize {
        critical_section::with(|cs| {
            self.inner
                .borrow_ref(cs)
                .regions
                .iter()
                .map(|region| region.heap.size())
                .sum()
        })
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use super::*;

    const THRESHOLD: usize = 4096;

    #[repr(C, align(128))]
    struct AlignedBacking<const SIZE: usize>([MaybeUninit<u8>; SIZE]);

    impl<const SIZE: usize> AlignedBacking<SIZE> {
        const fn uninit() -> Self {
            Self([const { MaybeUninit::uninit() }; SIZE])
        }

        fn init_region<const N: usize>(
            &'static mut self,
            manager: &RegionManager<N>,
            index: usize,
        ) -> bool {
            manager.init_region_from_slice(index, &mut self.0)
        }

        fn leak() -> &'static mut Self {
            alloc::boxed::Box::leak(alloc::boxed::Box::new(Self::uninit()))
        }
    }

    fn make_esp32s3_like_manager(
        internal: &'static mut AlignedBacking<{ 16 * 1024 }>,
        psram: Option<&'static mut AlignedBacking<{ 64 * 1024 }>>,
    ) -> RegionManager<2> {
        let manager = RegionManager::new(
            [
                RegionDescriptor::new(
                    CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible,
                )
                .with_dma_alignment(4),
                RegionDescriptor::new(CapabilityFlags::DirectMemoryAccess).with_dma_alignment(64),
            ],
            Some(THRESHOLD),
        );

        assert!(internal.init_region(&manager, 0));
        if let Some(psram) = psram {
            assert!(psram.init_region(&manager, 1));
        }

        manager
    }

    #[test]
    fn placement_policy_small_internal_large_psram() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));

        let small_layout = Layout::from_size_align(512, 8).unwrap();
        let large_layout = Layout::from_size_align(8192, 8).unwrap();

        let small_ptr =
            unsafe { manager.allocate(CapabilityFlags::empty(), small_layout) }.unwrap();
        let large_ptr =
            unsafe { manager.allocate(CapabilityFlags::empty(), large_layout) }.unwrap();

        assert!(manager.region_contains(0, small_ptr));
        assert!(!manager.region_contains(1, small_ptr));

        assert!(manager.region_contains(1, large_ptr));
        assert!(!manager.region_contains(0, large_ptr));

        unsafe {
            manager.deallocate(small_ptr, small_layout);
            manager.deallocate(large_ptr, large_layout);
        }

        assert_eq!(manager.get_used(), 0);
    }

    #[test]
    fn fallback_when_preferred_region_is_full_or_absent() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();

        // Without PSRAM initialized, large allocations fall back to internal SRAM.
        let internal_only = make_esp32s3_like_manager(internal, None);
        assert!(internal_only.is_region_initialized(0));
        assert!(!internal_only.is_region_initialized(1));

        let large_layout = Layout::from_size_align(8192, 8).unwrap();
        let ptr =
            unsafe { internal_only.allocate(CapabilityFlags::empty(), large_layout) }.unwrap();
        assert!(internal_only.region_contains(0, ptr));
        unsafe { internal_only.deallocate(ptr, large_layout) };

        // With both regions: exhausting internal falls back to PSRAM for small allocations.
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));
        let fill_layout = Layout::from_size_align(2048, 8).unwrap();
        let mut held = alloc::vec::Vec::new();

        loop {
            let p = unsafe { manager.allocate(CapabilityFlags::empty(), fill_layout) }.unwrap();
            let in_psram = manager.region_contains(1, p);
            held.push((p, fill_layout));
            if in_psram {
                break;
            }
            assert!(held.len() < 16);
        }

        // Exhaust PSRAM with a huge chunk and verify large allocations fall back to internal if free.
        for (p, l) in held.drain(..) {
            unsafe { manager.deallocate(p, l) };
        }
        assert_eq!(manager.get_used(), 0);

        let psram_fill = Layout::from_size_align(60 * 1024, 8).unwrap();
        let p_psram = unsafe { manager.allocate(CapabilityFlags::empty(), psram_fill) }.unwrap();
        assert!(manager.region_contains(1, p_psram));

        let large_fallback = Layout::from_size_align(8192, 8).unwrap();
        let p_fallback =
            unsafe { manager.allocate(CapabilityFlags::empty(), large_fallback) }.unwrap();
        assert!(manager.region_contains(0, p_fallback));

        unsafe {
            manager.deallocate(p_fallback, large_fallback);
            manager.deallocate(p_psram, psram_fill);
        }
        assert_eq!(manager.get_used(), 0);
    }

    #[test]
    fn capability_filtering_and_dma_alignment() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));

        // 1. AlwaysAccessible stays in internal SRAM even when >= threshold.
        let large_layout = Layout::from_size_align(8192, 64).unwrap();
        let always_acc =
            unsafe { manager.allocate(CapabilityFlags::AlwaysAccessible, large_layout) }.unwrap();
        assert!(manager.region_contains(0, always_acc));
        unsafe { manager.deallocate(always_acc, large_layout) };

        // 2. Executable is impossible on ESP32-like descriptors -> returns None.
        let small_layout = Layout::from_size_align(64, 8).unwrap();
        assert!(unsafe { manager.allocate(CapabilityFlags::Executable, small_layout) }.is_none());
        let invalid_dma = Layout::from_size_align(65, 64).unwrap();
        assert!(
            unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, invalid_dma) }.is_none()
        );
        let invalid_dma = Layout::from_size_align(64, 2).unwrap();
        assert!(
            unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, invalid_dma) }.is_none()
        );

        // 3. DMA alignment enforcement on PSRAM (64 B):
        //    - align=4, size=4096 -> PSRAM rejects (align < 64), internal accepts (align >= 4).
        let dma_unaligned = Layout::from_size_align(4096, 4).unwrap();
        let p1 = unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, dma_unaligned) }
            .unwrap();
        assert!(manager.region_contains(0, p1));

        //    - align=64, size=4100 (not multiple of 64) -> PSRAM rejects, internal accepts.
        let dma_bad_size = Layout::from_size_align(4100, 64).unwrap();
        let p2 =
            unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, dma_bad_size) }.unwrap();
        assert!(manager.region_contains(0, p2));

        //    - align=64, size=4096 -> PSRAM accepts and is preferred (>= threshold).
        let dma_aligned = Layout::from_size_align(4096, 64).unwrap();
        let p3 =
            unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, dma_aligned) }.unwrap();
        assert!(manager.region_contains(1, p3));
        assert_eq!(p3.as_ptr() as usize % 64, 0);

        unsafe {
            manager.deallocate(p1, dma_unaligned);
            manager.deallocate(p2, dma_bad_size);
            manager.deallocate(p3, dma_aligned);
        }

        // 4. Original ESP32 model: PSRAM has empty() capabilities -> DMA never lands in PSRAM.
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let esp32_mgr = RegionManager::new(
            [
                RegionDescriptor::new(
                    CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible,
                )
                .with_dma_alignment(4),
                RegionDescriptor::new(CapabilityFlags::empty()),
            ],
            Some(THRESHOLD),
        );
        assert!(internal.init_region(&esp32_mgr, 0));
        assert!(psram.init_region(&esp32_mgr, 1));

        let dma_large = Layout::from_size_align(8192, 64).unwrap();
        let p_dma =
            unsafe { esp32_mgr.allocate(CapabilityFlags::DirectMemoryAccess, dma_large) }.unwrap();
        assert!(esp32_mgr.region_contains(0, p_dma));
        unsafe { esp32_mgr.deallocate(p_dma, dma_large) };
    }

    const WORD: usize = size_of::<usize>();

    /// Bytes a block of `size` bytes occupies in a region.
    fn footprint(size: usize) -> usize {
        size.max(2 * WORD).next_multiple_of(WORD)
    }

    fn region_used<const N: usize>(manager: &RegionManager<N>, region: usize) -> usize {
        manager.get_statistics().region_stats[region]
            .as_ref()
            .unwrap()
            .used
    }

    /// Checks the free list invariants of every initialized region.
    fn validate_regions<const N: usize>(manager: &RegionManager<N>) {
        critical_section::with(|cs| {
            for region in manager.inner.borrow_ref(cs).regions.iter() {
                if !region.is_empty() {
                    region.heap.validate();
                }
            }
        });
    }

    fn stamp(pointer: NonNull<u8>, length: usize, seed: u8) {
        for i in 0..length {
            unsafe { pointer.as_ptr().add(i).write(seed.wrapping_add(i as u8)) };
        }
    }

    fn assert_stamp(pointer: NonNull<u8>, length: usize, seed: u8) {
        for i in 0..length {
            assert_eq!(
                unsafe { pointer.as_ptr().add(i).read() },
                seed.wrapping_add(i as u8),
                "byte {i} changed"
            );
        }
    }

    #[test]
    fn reallocate_prefers_the_old_region_and_spills_when_it_is_full() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));

        let small_layout = Layout::from_size_align(256, 8).unwrap();
        let large_layout = Layout::from_size_align(8192, 8).unwrap();

        // Start small -> internal SRAM.
        let p_small = unsafe { manager.allocate(CapabilityFlags::empty(), small_layout) }.unwrap();
        assert!(manager.region_contains(0, p_small));
        stamp(p_small, small_layout.size(), 1);

        // Same layout -> keeps exact pointer.
        let p_same = unsafe { manager.reallocate(p_small, small_layout, small_layout) }.unwrap();
        assert_eq!(p_same, p_small);

        // Growing past the threshold stays in the region that already holds the block.
        let p_large = unsafe { manager.reallocate(p_same, small_layout, large_layout) }.unwrap();
        assert!(manager.region_contains(0, p_large));
        assert_stamp(p_large, small_layout.size(), 1);
        assert_eq!(region_used(&manager, 1), 0);

        // Failed reallocations leave the old allocation intact.
        let too_large = Layout::from_size_align(128 * 1024, 128).unwrap();
        let zero = Layout::from_size_align(0, 8).unwrap();
        for layout in [too_large, zero] {
            assert!(unsafe { manager.reallocate(p_large, large_layout, layout) }.is_none());
        }
        assert_stamp(p_large, small_layout.size(), 1);
        assert_eq!(region_used(&manager, 0), footprint(large_layout.size()));

        // Shrinking never moves the block, even across the threshold.
        let p_shrunk = unsafe { manager.reallocate(p_large, large_layout, small_layout) }.unwrap();
        assert_eq!(p_shrunk, p_large);
        assert_stamp(p_shrunk, small_layout.size(), 1);
        assert_eq!(region_used(&manager, 0), footprint(small_layout.size()));

        // When the region cannot hold the grown block, it spills to the next region.
        let filler_layout = Layout::from_size_align(8192, 8).unwrap();
        let mut fillers = alloc::vec::Vec::new();
        while let Some(filler) =
            unsafe { manager.allocate(CapabilityFlags::AlwaysAccessible, filler_layout) }
        {
            fillers.push(filler);
        }
        assert!(!fillers.is_empty());
        let p_spilled =
            unsafe { manager.reallocate(p_shrunk, small_layout, large_layout) }.unwrap();
        assert!(manager.region_contains(1, p_spilled));
        assert_stamp(p_spilled, small_layout.size(), 1);
        assert_eq!(region_used(&manager, 1), footprint(large_layout.size()));

        for filler in fillers {
            unsafe { manager.deallocate(filler, filler_layout) };
        }
        unsafe { manager.deallocate(p_spilled, large_layout) };
        assert_eq!(manager.get_used(), 0);
        validate_regions(&manager);
    }

    #[test]
    fn shrink_keeps_the_pointer_and_frees_the_tail() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, None);

        let big = Layout::from_size_align(1024, 8).unwrap();
        let small = Layout::from_size_align(256, 8).unwrap();
        let guard = Layout::from_size_align(64, 8).unwrap();

        let block = unsafe { manager.allocate(CapabilityFlags::empty(), big) }.unwrap();
        let after = unsafe { manager.allocate(CapabilityFlags::empty(), guard) }.unwrap();
        stamp(block, small.size(), 7);
        let used = manager.get_used();
        let free = manager.get_free();

        let shrunk = unsafe { manager.reallocate(block, big, small) }.unwrap();
        assert_eq!(shrunk, block);
        assert_stamp(shrunk, small.size(), 7);
        assert_eq!(manager.get_used(), used - (1024 - 256));
        assert_eq!(manager.get_free(), free + (1024 - 256));

        // The tail is a hole again: the next allocation that fits lands in it.
        let tail_layout = Layout::from_size_align(512, 8).unwrap();
        let in_tail = unsafe { manager.allocate(CapabilityFlags::empty(), tail_layout) }.unwrap();
        assert_eq!(in_tail.as_ptr(), unsafe { block.as_ptr().add(256) });
        validate_regions(&manager);

        unsafe {
            manager.deallocate(in_tail, tail_layout);
            manager.deallocate(shrunk, small);
            manager.deallocate(after, guard);
        }
        assert_eq!(manager.get_used(), 0);
        validate_regions(&manager);
    }

    #[test]
    fn grow_in_place_when_the_next_block_is_free() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, None);

        let small = Layout::from_size_align(256, 8).unwrap();
        let big = Layout::from_size_align(1000, 8).unwrap();

        let block = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
        stamp(block, small.size(), 3);
        let used = manager.get_used();

        let grown = unsafe { manager.reallocate(block, small, big) }.unwrap();
        assert_eq!(grown, block);
        assert_stamp(grown, small.size(), 3);
        assert_eq!(manager.get_used(), used + footprint(1000) - footprint(256));
        validate_regions(&manager);

        // A neighbour that was freed is reclaimed too.
        let neighbour = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
        assert_eq!(neighbour.as_ptr(), unsafe {
            block.as_ptr().add(footprint(1000))
        });
        let bigger = Layout::from_size_align(1200, 8).unwrap();
        let moved = unsafe { manager.reallocate(grown, big, bigger) }.unwrap();
        assert_ne!(moved, grown);
        unsafe { manager.deallocate(neighbour, small) };
        let regrown =
            unsafe { manager.reallocate(moved, bigger, Layout::from_size_align(2000, 8).unwrap()) }
                .unwrap();
        assert_eq!(regrown, moved);

        unsafe { manager.deallocate(regrown, Layout::from_size_align(2000, 8).unwrap()) };
        assert_eq!(manager.get_used(), 0);
        validate_regions(&manager);
    }

    #[test]
    fn grow_moves_when_the_next_block_is_busy() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, None);

        let small = Layout::from_size_align(256, 8).unwrap();
        let big = Layout::from_size_align(512, 8).unwrap();

        let block = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
        let neighbour = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
        stamp(block, small.size(), 11);
        stamp(neighbour, small.size(), 99);
        assert_eq!(neighbour.as_ptr(), unsafe { block.as_ptr().add(256) });

        let moved = unsafe { manager.reallocate(block, small, big) }.unwrap();
        assert_ne!(moved, block);
        assert_stamp(moved, small.size(), 11);
        assert_stamp(neighbour, small.size(), 99);
        assert_eq!(manager.get_used(), footprint(512) + footprint(256));
        validate_regions(&manager);

        // The old block went back to the free list and is reused.
        let reused = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
        assert_eq!(reused, block);

        unsafe {
            manager.deallocate(reused, small);
            manager.deallocate(moved, big);
            manager.deallocate(neighbour, small);
        }
        assert_eq!(manager.get_used(), 0);
    }

    #[test]
    fn reallocation_keeps_capabilities_and_alignment() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));
        let blocker = Layout::from_size_align(64, 8).unwrap();

        // `AlwaysAccessible` data grown past the threshold stays in internal SRAM, whether the
        // block is resized in place or has to move.
        for move_it in [false, true] {
            let small = Layout::from_size_align(1024, 8).unwrap();
            let large = Layout::from_size_align(8192, 8).unwrap();
            let block =
                unsafe { manager.allocate(CapabilityFlags::AlwaysAccessible, small) }.unwrap();
            let guard = move_it.then(|| unsafe {
                manager
                    .allocate(CapabilityFlags::AlwaysAccessible, blocker)
                    .unwrap()
            });
            stamp(block, small.size(), 5);

            let grown = unsafe { manager.reallocate(block, small, large) }.unwrap();
            assert_eq!(grown == block, !move_it);
            assert!(manager.region_contains(0, grown));
            assert_stamp(grown, small.size(), 5);

            unsafe { manager.deallocate(grown, large) };
            if let Some(guard) = guard {
                unsafe { manager.deallocate(guard, blocker) };
            }
            assert_eq!(manager.get_used(), 0);
        }

        // 64-byte DMA data in PSRAM keeps its region and alignment. The guard is large enough to be
        // placed in PSRAM, right after the block.
        let psram_guard = Layout::from_size_align(THRESHOLD, 64).unwrap();
        for move_it in [false, true] {
            let small = Layout::from_size_align(4096, 64).unwrap();
            let large = Layout::from_size_align(8192, 64).unwrap();
            let block =
                unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, small) }.unwrap();
            assert!(manager.region_contains(1, block));
            let guard = move_it.then(|| unsafe {
                manager
                    .allocate(CapabilityFlags::DirectMemoryAccess, psram_guard)
                    .unwrap()
            });
            stamp(block, small.size(), 9);

            let grown = unsafe { manager.reallocate(block, small, large) }.unwrap();
            assert_eq!(grown == block, !move_it);
            assert!(manager.region_contains(1, grown));
            assert_eq!(grown.as_ptr().addr() % 64, 0);
            assert_stamp(grown, small.size(), 9);

            // A move keeps the alignment even when the new layout asks for less.
            let relaxed = Layout::from_size_align(16384, 8).unwrap();
            let guard2 =
                unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, psram_guard) }
                    .unwrap();
            let regrown = unsafe { manager.reallocate(grown, large, relaxed) }.unwrap();
            assert!(manager.region_contains(1, regrown));
            assert_eq!(regrown.as_ptr().addr() % 64, 0);
            assert_stamp(regrown, small.size(), 9);

            unsafe {
                manager.deallocate(regrown, relaxed);
                manager.deallocate(guard2, psram_guard);
                if let Some(guard) = guard {
                    manager.deallocate(guard, psram_guard);
                }
            }
            assert_eq!(manager.get_used(), 0);
        }
        validate_regions(&manager);
    }

    #[test]
    fn dma_alignment_128_survives_reallocation() {
        let first = AlignedBacking::<{ 16 * 1024 }>::leak();
        let second = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = RegionManager::new(
            [
                RegionDescriptor::new(CapabilityFlags::DirectMemoryAccess).with_dma_alignment(64),
                RegionDescriptor::new(CapabilityFlags::DirectMemoryAccess).with_dma_alignment(128),
            ],
            Some(THRESHOLD),
        );
        assert!(first.init_region(&manager, 0));
        assert!(second.init_region(&manager, 1));

        let dma = Layout::from_size_align(8192, 128).unwrap();
        let block = unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, dma) }.unwrap();
        assert!(manager.region_contains(1, block));
        assert_eq!(block.as_ptr().addr() % 128, 0);

        // Grow and shrink repeatedly, with and without a neighbour in the way.
        let mut layout = dma;
        let mut pointer = block;
        let mut guards = alloc::vec::Vec::new();
        stamp(pointer, 256, 21);
        for (step, size) in [8320usize, 12288, 8192, 4096, 640, 128, 20480]
            .into_iter()
            .enumerate()
        {
            let next = Layout::from_size_align(size, 128).unwrap();
            pointer = unsafe { manager.reallocate(pointer, layout, next) }.unwrap();
            layout = next;
            assert!(
                manager.region_contains(1, pointer),
                "step {step} left the region"
            );
            assert_eq!(
                pointer.as_ptr().addr() % 128,
                0,
                "step {step} lost alignment"
            );
            assert_stamp(pointer, 128, 21);
            let guard_layout = Layout::from_size_align(128, 128).unwrap();
            guards.push(
                unsafe { manager.allocate(CapabilityFlags::DirectMemoryAccess, guard_layout) }
                    .unwrap(),
            );
        }
        for guard in guards {
            unsafe { manager.deallocate(guard, Layout::from_size_align(128, 128).unwrap()) };
        }
        unsafe { manager.deallocate(pointer, layout) };
        assert_eq!(manager.get_used(), 0);
        validate_regions(&manager);
    }

    /// Deterministic generator for the property test.
    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, bound: usize) -> usize {
            (self.next() % bound as u64) as usize
        }
    }

    struct Live {
        pointer: NonNull<u8>,
        layout: Layout,
        capabilities: CapabilityFlags,
        region: usize,
        seed: u8,
    }

    fn region_of(manager: &RegionManager<2>, pointer: NonNull<u8>) -> usize {
        (0..2)
            .find(|index| manager.region_contains(*index, pointer))
            .unwrap()
    }

    fn random_reallocation_sequences(seed: u64) {
        let internal = AlignedBacking::<{ 192 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 192 * 1024 }>::leak();
        let manager = RegionManager::new(
            [
                RegionDescriptor::new(
                    CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible,
                )
                .with_dma_alignment(4),
                RegionDescriptor::new(CapabilityFlags::DirectMemoryAccess).with_dma_alignment(64),
            ],
            Some(THRESHOLD),
        );
        assert!(internal.init_region(&manager, 0));
        assert!(psram.init_region(&manager, 1));

        let mut random = XorShift(seed);
        let mut live: alloc::vec::Vec<Live> = alloc::vec::Vec::new();
        let mut expected_used = 0usize;
        let mut next_seed = 0u8;
        let (mut grown_in_place, mut moved, mut shrunk, mut word_tail_moves) =
            (0usize, 0usize, 0usize, 0usize);

        // DMA data keeps sizes and alignment valid for every region, plain data does not.
        let make_layout = |random: &mut XorShift, capabilities: CapabilityFlags| {
            if capabilities.contains(CapabilityFlags::DirectMemoryAccess) {
                (random.below(24) + 1) * 64
            } else {
                random.below(1500) + 1
            }
        };

        for _ in 0..4000 {
            let before_used = manager.get_used();
            match random.below(10) {
                0..=3 if live.len() < 60 => {
                    let capabilities = [
                        CapabilityFlags::empty(),
                        CapabilityFlags::AlwaysAccessible,
                        CapabilityFlags::DirectMemoryAccess,
                    ][random.below(3)];
                    let size = make_layout(&mut random, capabilities);
                    let align = if capabilities.contains(CapabilityFlags::DirectMemoryAccess) {
                        64
                    } else {
                        1 << random.below(6)
                    };
                    let layout = Layout::from_size_align(size, align).unwrap();
                    let Some(pointer) = (unsafe { manager.allocate(capabilities, layout) }) else {
                        continue;
                    };
                    next_seed = next_seed.wrapping_add(37);
                    stamp(pointer, size, next_seed);
                    expected_used += footprint(size);
                    live.push(Live {
                        pointer,
                        layout,
                        capabilities,
                        region: region_of(&manager, pointer),
                        seed: next_seed,
                    });
                    assert_eq!(pointer.as_ptr().addr() % align, 0);
                }
                4..=5 if !live.is_empty() => {
                    let block = live.swap_remove(random.below(live.len()));
                    unsafe { manager.deallocate(block.pointer, block.layout) };
                    expected_used -= footprint(block.layout.size());
                }
                _ if !live.is_empty() => {
                    let index = random.below(live.len());
                    let block = &mut live[index];
                    // Mostly small steps, so that in-place growth and one-word shrinks happen.
                    let unit = if block
                        .capabilities
                        .contains(CapabilityFlags::DirectMemoryAccess)
                    {
                        64
                    } else {
                        WORD
                    };
                    let new_size = match random.below(4) {
                        0 => make_layout(&mut random, block.capabilities),
                        1 => block.layout.size() + (random.below(8) + 1) * unit,
                        2 => block
                            .layout
                            .size()
                            .saturating_sub((random.below(8) + 1) * unit),
                        _ => block.layout.size().saturating_sub(WORD),
                    }
                    .max(unit);
                    let new_layout =
                        Layout::from_size_align(new_size, block.layout.align()).unwrap();

                    let old_pointer = block.pointer;
                    let old_size = block.layout.size();
                    let Some(pointer) =
                        (unsafe { manager.reallocate(old_pointer, block.layout, new_layout) })
                    else {
                        // A failed reallocation changes nothing.
                        assert_eq!(manager.get_used(), before_used);
                        assert_stamp(old_pointer, old_size, block.seed);
                        continue;
                    };

                    assert_stamp(pointer, old_size.min(new_size), block.seed);
                    assert_eq!(pointer.as_ptr().addr() % block.layout.align(), 0);
                    if new_size <= old_size && footprint(old_size) - footprint(new_size) != WORD {
                        assert_eq!(pointer, old_pointer, "a shrink must not move");
                    }
                    let region = region_of(&manager, pointer);
                    assert_eq!(region, block.region, "the block left its region");
                    assert!(
                        manager.is_region_initialized(region)
                            && critical_section::with(|cs| manager.inner.borrow_ref(cs).regions
                                [region]
                                .supports(block.capabilities, new_layout)),
                        "capabilities or DMA alignment lost"
                    );

                    expected_used = expected_used - footprint(old_size) + footprint(new_size);
                    if new_size > old_size {
                        if pointer == old_pointer {
                            grown_in_place += 1;
                        } else {
                            moved += 1;
                        }
                    } else if new_size < old_size {
                        shrunk += 1;
                        if pointer != old_pointer {
                            word_tail_moves += 1;
                        }
                    }

                    block.pointer = pointer;
                    block.layout = new_layout;
                    next_seed = next_seed.wrapping_add(37);
                    block.seed = next_seed;
                    stamp(pointer, new_size, next_seed);
                }
                _ => {}
            }

            assert_eq!(manager.get_used(), expected_used);
            let statistics = manager.get_statistics();
            for region in statistics.region_stats.iter().flatten() {
                assert_eq!(region.used + region.free, region.size);
            }
            #[cfg(feature = "Debug")]
            assert_eq!(
                statistics.total_allocated - statistics.total_freed,
                statistics.current_usage
            );
        }

        // The sequence exercised every path.
        assert!(
            grown_in_place > 20 && moved > 20 && shrunk > 20,
            "{grown_in_place} {moved} {shrunk}"
        );
        assert!(word_tail_moves > 0, "no one-word shrink was exercised");

        // Nothing overlaps and nothing was overwritten.
        live.sort_by_key(|block| block.pointer.as_ptr().addr());
        for pair in live.windows(2) {
            assert!(
                pair[0].pointer.as_ptr().addr() + footprint(pair[0].layout.size())
                    <= pair[1].pointer.as_ptr().addr()
            );
        }
        for block in &live {
            assert_stamp(block.pointer, block.layout.size(), block.seed);
        }
        validate_regions(&manager);

        for block in live {
            unsafe { manager.deallocate(block.pointer, block.layout) };
        }
        assert_eq!(manager.get_used(), 0);
        validate_regions(&manager);
    }

    #[test]
    fn random_reallocation_sequences_keep_statistics_exact() {
        for seed in [
            0x9E37_79B9_7F4A_7C15,
            0xD1B5_4A32_D192_ED03,
            0x1234_5678_9ABC_DEF1,
            0xFEED_FACE_CAFE_BEEF,
        ] {
            random_reallocation_sequences(seed);
        }
    }

    #[test]
    fn per_region_statistics_and_edge_cases() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));

        // Zero-sized allocation returns None.
        let zero = Layout::from_size_align(0, 1).unwrap();
        assert!(unsafe { manager.allocate(CapabilityFlags::empty(), zero) }.is_none());

        // Null / tiny init returns false without panicking.
        let uninit_mgr =
            RegionManager::new([RegionDescriptor::new(CapabilityFlags::empty())], None);
        assert!(unsafe { !uninit_mgr.init_region(0, core::ptr::null_mut(), 0) });
        let tiny = alloc::boxed::Box::leak(alloc::boxed::Box::<[u8]>::new_uninit_slice(4));
        assert!(!uninit_mgr.init_region_from_slice(0, tiny));

        let l1 = Layout::from_size_align(1024, 16).unwrap();
        let l2 = Layout::from_size_align(8192, 64).unwrap();
        let p1 = unsafe { manager.allocate(CapabilityFlags::empty(), l1) }.unwrap();
        let p2 = unsafe { manager.allocate(CapabilityFlags::empty(), l2) }.unwrap();

        let stats = manager.get_statistics();
        let r0 = stats.region_stats[0].as_ref().unwrap();
        let r1 = stats.region_stats[1].as_ref().unwrap();
        assert!(r0.used >= 1024);
        assert!(r1.used >= 8192);
        assert_eq!(p2.as_ptr().addr() % 64, 0);
        assert_eq!(r0.used + r0.free, r0.size);
        assert_eq!(r1.used + r1.free, r1.size);
        assert_eq!(stats.current_usage, r0.used + r1.used);
        assert_eq!(stats.size, r0.size + r1.size);
        assert_eq!(
            manager.get_free_for_capabilities(CapabilityFlags::AlwaysAccessible),
            r0.free
        );
        assert_eq!(
            manager.get_free_for_capabilities(CapabilityFlags::DirectMemoryAccess),
            r0.free + r1.free
        );

        unsafe {
            manager.deallocate(p1, l1);
            manager.deallocate(p2, l2);
        }

        let after = manager.get_statistics();
        assert_eq!(after.current_usage, 0);
        #[cfg(feature = "Debug")]
        {
            assert!(after.max_usage >= 1024 + 8192);
            assert_eq!(after.total_allocated, after.total_freed);
            assert!(after.total_allocated >= 1024 + 8192);
        }
    }

    #[test]
    fn report_struct_sizes() {
        // Sanity bound: on 64-bit host, 2 heaps (48B each) + descriptors + mutex + optional Debug stats.
        assert!(size_of::<RegionManager<2>>() <= 160);
    }

    mod memory_trait_suite {
        use super::*;

        fn build_test_manager() -> RegionManager<2> {
            let internal = alloc::boxed::Box::leak(alloc::boxed::Box::<[u8]>::new_uninit_slice(
                2 * 1024 * 1024,
            ));
            let external = alloc::boxed::Box::leak(alloc::boxed::Box::<[u8]>::new_uninit_slice(
                2 * 1024 * 1024,
            ));

            let manager = RegionManager::new(
                [
                    RegionDescriptor::new(
                        CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible,
                    ),
                    RegionDescriptor::new(CapabilityFlags::Executable),
                ],
                Some(4096),
            );
            assert!(manager.init_region_from_slice(0, internal));
            assert!(manager.init_region_from_slice(1, external));
            manager
        }

        crate::implement_memory_manager_tests!(
            [#[test]], build_test_manager(),
            capabilities = CapabilityFlags::all()
        );
    }
}
