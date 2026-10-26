use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};
use timezone::{CivilDateTime, LocalResolution, Zone};

struct CountAllocations;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountAllocations {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAllocations = CountAllocations;

#[test]
fn static_lookup_conversion_and_error_paths_do_not_allocate() {
    let local = CivilDateTime::new(2024, 10, 27, 2, 30, 0, 123).unwrap();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for id in timezone::AVAILABLE_ZONE_IDS {
        let zone = Zone::get(id).unwrap();
        std::hint::black_box(zone.utc_to_local(1_729_989_000_000_000_000).unwrap());
        match zone.local_to_utc(local).unwrap() {
            LocalResolution::Unique(value) => {
                std::hint::black_box(value);
            }
            result => {
                std::hint::black_box(result);
            }
        }
        assert!(zone.utc_to_local(i128::MAX).is_err());
    }
    assert!(Zone::get("unavailable").is_err());
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), before);
}
