//! Thread-local allocation accounting excludes other concurrently running tests.
use core::cell::Cell;
use std::alloc::{GlobalAlloc, Layout, System};

std::thread_local! {
    static COUNTS: Cell<Option<(usize, isize)>> = const { Cell::new(None) };
}

struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn record(allocations: usize, bytes: isize) {
    let _ = COUNTS.try_with(|counts| {
        if let Some((count, live)) = counts.get() {
            counts.set(Some((count + allocations, live + bytes)));
        }
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record(1, layout.size() as isize);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record(0, -(layout.size() as isize));
        unsafe { System.dealloc(pointer, layout) };
    }
}

pub fn start() {
    COUNTS.set(Some((0, 0)));
}
pub fn finish() -> (usize, isize) {
    COUNTS.replace(None).unwrap()
}
