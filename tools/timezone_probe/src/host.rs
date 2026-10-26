use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Counter;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

fn main() {
    let seconds = std::env::args()
        .nth(1)
        .map(|value| value.parse().unwrap())
        .unwrap_or(1_729_989_000);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut checksum = 0i64;
    for index in 0..6 {
        checksum = checksum.wrapping_add(timezone_probe::convert(
            std::hint::black_box(seconds),
            index,
        ));
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    println!("checksum={checksum} conversion_allocations={allocations}");
    assert_eq!(allocations, 0);
    #[cfg(feature = "utc")]
    println!(
        "Zone={} LocalTime={} LocalResolution={} bytes",
        core::mem::size_of::<timezone::Zone>(),
        core::mem::size_of::<timezone::LocalTime>(),
        core::mem::size_of::<timezone::LocalResolution>()
    );
}
