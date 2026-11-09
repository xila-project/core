//! Route Rust allocations through the Xila memory manager, which LittleFS and
//! the ABI use through the exported `__XILA_MEMORY_MANAGER` symbol.
use std::alloc::{GlobalAlloc, Layout};

static NATIVE: testing::MemoryManager = testing::MemoryManager::new();

#[unsafe(no_mangle)]
pub static __XILA_MEMORY_MANAGER: testing::memory_crate::Manager<'static> =
    testing::memory_crate::Manager::new(&NATIVE);

struct Allocator;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { __XILA_MEMORY_MANAGER.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { __XILA_MEMORY_MANAGER.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        unsafe { __XILA_MEMORY_MANAGER.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { __XILA_MEMORY_MANAGER.dealloc(ptr, layout) }
    }
}
