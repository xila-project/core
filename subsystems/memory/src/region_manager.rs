use core::{
    cell::RefCell,
    mem::MaybeUninit,
    ptr::{NonNull, copy_nonoverlapping},
};

use critical_section::Mutex;
use linked_list_allocator::Heap;

use crate::{CapabilityFlags, Layout, ManagerTrait, RegionStatistics, Statistics};

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

    fn supports(&self, capabilities: CapabilityFlags, layout: Layout) -> bool {
        if self.is_empty() || !self.descriptor.capabilities.contains(capabilities) {
            return false;
        }

        if capabilities.contains(CapabilityFlags::DirectMemoryAccess) {
            let alignment = 1usize << self.descriptor.dma_alignment_shift;
            return layout.align() >= alignment && layout.size().is_multiple_of(alignment);
        }

        true
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
        region.heap.allocate_first_fit(layout).ok()
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

        let region = self
            .regions
            .iter_mut()
            .find(|region| region.contains(pointer, layout))
            .expect("Pointer does not belong to any managed memory region");

        unsafe { region.heap.deallocate(pointer, layout) };

        #[cfg(feature = "Debug")]
        {
            let used_after = self.total_used();
            let delta = used_before.saturating_sub(used_after);
            self.stats.total_freed = self.stats.total_freed.saturating_add(delta);
        }
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

    unsafe fn reallocate(
        &self,
        old_pointer: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Option<NonNull<u8>> {
        if old_layout == new_layout && new_layout.size() > 0 {
            return Some(old_pointer);
        }

        let new_pointer = unsafe { self.allocate(CapabilityFlags::empty(), new_layout) }?;

        let old_size = old_layout.size();
        let new_size = new_layout.size();
        if old_size > 0 && new_size > 0 {
            let copy_size = core::cmp::min(old_size, new_size);
            unsafe {
                copy_nonoverlapping(old_pointer.as_ptr(), new_pointer.as_ptr(), copy_size);
            }
        }

        unsafe { self.deallocate(old_pointer, old_layout) };

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

    #[test]
    fn reallocate_across_regions_preserves_data() {
        let internal = AlignedBacking::<{ 16 * 1024 }>::leak();
        let psram = AlignedBacking::<{ 64 * 1024 }>::leak();
        let manager = make_esp32s3_like_manager(internal, Some(psram));

        let small_layout = Layout::from_size_align(256, 8).unwrap();
        let large_layout = Layout::from_size_align(8192, 8).unwrap();

        // Start small -> internal SRAM.
        let p_small = unsafe { manager.allocate(CapabilityFlags::empty(), small_layout) }.unwrap();
        assert!(manager.region_contains(0, p_small));
        unsafe {
            for i in 0..small_layout.size() {
                *p_small.as_ptr().add(i) = (i & 0xff) as u8;
            }
        }

        // Same layout -> keeps exact pointer.
        let p_same = unsafe { manager.reallocate(p_small, small_layout, small_layout) }.unwrap();
        assert_eq!(p_same, p_small);

        // Grow past threshold -> moves to PSRAM and frees internal SRAM.
        let p_large = unsafe { manager.reallocate(p_same, small_layout, large_layout) }.unwrap();
        assert!(manager.region_contains(1, p_large));
        unsafe {
            for i in 0..small_layout.size() {
                assert_eq!(*p_large.as_ptr().add(i), (i & 0xff) as u8);
            }
        }
        assert_eq!(
            manager.get_statistics().region_stats[0]
                .as_ref()
                .unwrap()
                .used,
            0
        );

        // Failed reallocations leave the old allocation intact.
        let too_large = Layout::from_size_align(128 * 1024, 128).unwrap();
        let zero = Layout::from_size_align(0, 8).unwrap();
        for layout in [too_large, zero] {
            assert!(unsafe { manager.reallocate(p_large, large_layout, layout) }.is_none());
        }

        // Shrink below threshold -> moves back to internal SRAM and frees PSRAM.
        let p_shrunk = unsafe { manager.reallocate(p_large, large_layout, small_layout) }.unwrap();
        assert!(manager.region_contains(0, p_shrunk));
        unsafe {
            for i in 0..small_layout.size() {
                assert_eq!(*p_shrunk.as_ptr().add(i), (i & 0xff) as u8);
            }
        }
        assert_eq!(
            manager.get_statistics().region_stats[1]
                .as_ref()
                .unwrap()
                .used,
            0
        );

        unsafe { manager.deallocate(p_shrunk, small_layout) };
        assert_eq!(manager.get_used(), 0);
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
