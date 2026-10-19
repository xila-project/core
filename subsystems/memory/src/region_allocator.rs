/* use core::{
    alloc::{GlobalAlloc, Layout},
    cell::{Ref, RefCell},
    ptr::NonNull,
};

use critical_section::Mutex;
use linked_list_allocator::Heap;

use crate::{Allocator_trait, Capabilities_type, Region_statistics_type, Statistics_type};

#[cfg(feature = "Debug")]
struct Internal_heap_statistics {
    max_usage: usize,
    total_allocated: usize,
    total_freed: usize,
}

struct Region_type {
    pub Heap: Heap,
    pub Capabilities: Capabilities_type,
}

impl Region_type {
    pub const fn New(Heap: Heap, Capabilities: Capabilities_type) -> Self {
        Self { Heap, Capabilities }
    }

    pub fn Initialize(&mut self, Start: usize, Size: usize) {
        self.Heap.initialize(Start, Size);
    }
}

/// A memory allocator
///
/// In addition to what Rust's memory allocator can do it allows to allocate
/// memory in regions satisfying specific needs.
pub struct Region_allocator_type<const Regions_count: usize> {
    Regions: Mutex<RefCell<[Region_type; Regions_count]>>,
    #[cfg(feature = "Debug")]
    internal_heap_stats: Mutex<RefCell<Internal_heap_statistics>>,
}

impl<const Regions: usize> Allocator_trait for Region_allocator_type<Regions> {
    unsafe fn Allocate(&self, Capabilities: Capabilities_type, Layout: Layout) -> *mut u8 {
        self.Allocate(Capabilities, Layout)
    }

    unsafe fn Deallocate(&self, Pointer: *mut u8, Layout: Layout) {
        self.Deallocate(Pointer, Layout)
    }

    unsafe fn get_statistics(&self) -> Statistics_type {
        self.get_statistics()
    }
}

impl<const Regions_count: usize> Region_allocator_type<Regions_count> {
    /// Create a new allocator
    pub const fn New(Regions: [Region_type; Regions_count]) -> Self {
        Self {
            Regions: Mutex::new(RefCell::new(Regions)),
            #[cfg(feature = "Debug")]
            internal_heap_stats: Mutex::const_new(RefCell::new(Internal_heap_statistics {
                max_usage: 0,
                total_allocated: 0,
                total_freed: 0,
            })),
        }
    }

    pub fn get_statistics(&self) -> Statistics_type {
        const EMPTY_REGION_STAT: Option<Region_statistics_type> = None;
        let mut region_stats: [Option<Region_statistics_type>; 3] = [EMPTY_REGION_STAT; 3];

        critical_section::with(|Critical_section| {
            let mut used = 0;
            let mut free = 0;
            let regions = self.Regions.borrow_ref(Critical_section);
            for (id, region) in regions.iter().enumerate() {
                if let Some(region) = region.as_ref() {
                    let stats = region.stats();
                    free += stats.free;
                    used += stats.used;
                    region_stats[id] = Some(region.stats());
                }
            }

            #[cfg(feature = "Debug")]
            {
                let internal_heap_stats = self.internal_heap_stats.borrow_ref(cs);
                HeapStats {
                    region_stats,
                    size: free + used,
                    current_usage: used,
                    max_usage: internal_heap_stats.max_usage,
                    total_allocated: internal_heap_stats.total_allocated,
                    total_freed: internal_heap_stats.total_freed,
                }
            }
            #[cfg(not(feature = "Debug"))]
            {
                HeapStats {
                    region_stats,
                    size: free + used,
                    current_usage: used,
                }
            }
        })
    }

    /// The free heap satisfying the given requirements
    pub fn Deallocate(&self, Capabilities: Capabilities_type) -> usize {
        critical_section::with(|cs| {
            let Regions = self.Regions.borrow(cs);
            let mut free = 0;
            for Region in Regions.iter().filter(|Region| {
                if Region.is_some() {
                    Region
                        .as_ref()
                        .unwrap()
                        .capabilities
                        .is_superset(Capabilities)
                } else {
                    false
                }
            }) {
                if let Some(region) = Region.as_ref() {
                    free += region.Heap.free();
                }
            }
            free
        })
    }

    pub unsafe fn Allocate(&self, capabilities: Capabilities_type, layout: Layout) -> *mut u8 {
        critical_section::with(|Critical_section| {
            #[cfg(feature = "Debug")]
            let before = self.used();

            let Result = self
                .Regions
                .borrow_ref_mut(Critical_section)
                .iter_mut()
                .find_map(|Region| {
                    if Region.is_none() {
                        return None;
                    }

                    let Region = Region.as_mut().unwrap();
                    if Region.get_capabilities().is_superset(capabilities) {
                        Region.Allocate(layout)
                    } else {
                        None
                    }
                });

            Result.map(|Allocation| {
                #[cfg(feature = "Debug")]
                {
                    let mut internal_heap_stats =
                        self.internal_heap_stats.borrow_ref_mut(Critical_section);
                    drop(Region_type);
                    // We need to call used because [linked_list_allocator::Heap] does internal size
                    // alignment so we cannot use the size provided by the layout.
                    let used = self.used();

                    internal_heap_stats.total_allocated += used - before;
                    internal_heap_stats.max_usage =
                        core::cmp::max(internal_heap_stats.max_usage, used);
                }

                Allocation
            })
        })
    }
}

unsafe impl GlobalAlloc for Region_allocator_type {
    unsafe fn alloc(&self, Layout: Layout) -> *mut u8 {
        self.Allocate(Capabilities_type::new(false, false), Layout)
    }

    unsafe fn dealloc(&self, Pointer: *mut u8, Layout: Layout) {
        if Pointer.is_null() {
            return;
        }

        critical_section::with(|cs| {
            #[cfg(feature = "Debug")]
            let before = self.used();
            let mut regions = self.Regions.borrow_ref_mut(cs);
            let mut iter = (*regions).iter_mut();

            while let Some(Some(region)) = iter.next() {
                if region.Heap.bottom() <= Pointer && region.Heap.top() >= Pointer {
                    region
                        .Heap
                        .deallocate(NonNull::new_unchecked(Pointer), Layout);
                }
            }

            #[cfg(feature = "Debug")]
            {
                let mut internal_heap_stats = self.internal_heap_stats.borrow_ref_mut(cs);
                drop(regions);
                // We need to call `used()` because [linked_list_allocator::Heap] does internal
                // size alignment so we cannot use the size provided by the
                // layout.
                internal_heap_stats.total_freed += before - self.used();
            }
        })
    }
}

#[cfg(feature = "Nightly")]
unsafe impl core::alloc::Allocator for Region_allocator_type {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, core::alloc::AllocError> {
        let raw_ptr = unsafe { self.alloc(layout) };

        if raw_ptr.is_null() {
            return Err(core::alloc::AllocError);
        }

        let ptr = NonNull::new(raw_ptr).ok_or(core::alloc::AllocError)?;
        Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        self.dealloc(ptr.as_ptr(), layout);
    }
}
*/
