//! Count Rust allocator requests only on the measuring test's thread, preserving
//! the Xila allocator symbol used by the ABI and LittleFS.
use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
};
static NATIVE: drivers_std::memory::MemoryManager = drivers_std::memory::MemoryManager::new();
#[unsafe(no_mangle)]
pub static __XILA_MEMORY_MANAGER: drivers_std::memory_exported::Manager<'static> =
    drivers_std::memory_exported::Manager::new(&NATIVE);

std::thread_local! { static REQUESTS: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Tracking;
#[global_allocator]
static ALLOCATOR: Tracking = Tracking;
fn record() {
    let _ = REQUESTS.try_with(|requests| {
        if let Some(count) = requests.get() {
            requests.set(Some(count + 1));
        }
    });
}
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { __XILA_MEMORY_MANAGER.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { __XILA_MEMORY_MANAGER.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        unsafe { __XILA_MEMORY_MANAGER.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { __XILA_MEMORY_MANAGER.dealloc(ptr, layout) }
    }
}
pub fn measure(operation: impl FnOnce()) -> usize {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            REQUESTS.with(|requests| requests.set(None));
        }
    }
    REQUESTS.with(|requests| {
        assert!(requests.get().is_none());
        requests.set(Some(0));
    });
    let reset = Reset;
    operation();
    let result = REQUESTS.with(|requests| requests.get().unwrap());
    drop(reset);
    result
}
