#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use core::{
    alloc::{GlobalAlloc, Layout},
    panic::PanicInfo,
};
use xila_sys::{Status, XilaStartup, runtime};

xila_sys::entry!(main);

#[global_allocator]
static HEAP: runtime::ProcessAllocator = runtime::ProcessAllocator;

#[panic_handler]
fn panic(_: &PanicInfo<'_>) -> ! {
    runtime::panic_abort()
}

#[cfg(feature = "lifecycle-test")]
mod lifecycle;

/// Called by the entry shim with validated, host-owned startup data.
unsafe fn main(startup: *const XilaStartup) -> i32 {
    #[cfg(feature = "lifecycle-test")]
    if lifecycle::INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) != 2 {
        return 1;
    }
    let Some(api) = runtime::api() else {
        return 1;
    };
    let mut now = 0;
    if unsafe { (api.time_monotonic)(&mut now) } != Status::Ok {
        return 1;
    }

    // Grow a Vec past its initial capacity and check that reallocation kept its data.
    let mut bytes = Vec::with_capacity(4);
    bytes.extend_from_slice(b"Xila");
    bytes.reserve_exact(bytes.capacity());
    if bytes.capacity() <= 4 || bytes.as_slice() != b"Xila" {
        return 2;
    }
    drop(bytes);

    // Exercise the allocator's over-aligned allocation and reallocation paths.
    let layout = Layout::from_size_align(32, 64).unwrap();
    let memory = unsafe { HEAP.alloc(layout) };
    if memory.is_null() {
        return 2;
    }
    let aligned = memory.addr().is_multiple_of(64);
    unsafe { memory.write(42) };
    let resized = unsafe { HEAP.realloc(memory, layout, 128) };
    if resized.is_null() {
        unsafe { HEAP.dealloc(memory, layout) };
        return 2;
    }
    let aligned = aligned && resized.addr().is_multiple_of(64);
    let preserved = unsafe { resized.read() } == 42;
    unsafe { HEAP.dealloc(resized, Layout::from_size_align(128, 64).unwrap()) };
    if !aligned || !preserved {
        return 2;
    }

    // The host test passes one argument to exercise the panic handler.
    if unsafe { (*startup).argument_count } != 0 {
        panic!("requested by the host test");
    }

    if write_output(b"Hello from Rust on Xila!\n") {
        73
    } else {
        3
    }
}

fn write_output(mut text: &[u8]) -> bool {
    let (Some(api), Some(startup)) = (runtime::api(), runtime::startup()) else {
        return false;
    };
    while !text.is_empty() {
        let mut written = 0;
        let result = unsafe {
            (api.file_write)(
                startup.standard_output,
                text.as_ptr().cast(),
                text.len(),
                &mut written,
            )
        };
        if result != Status::Ok || written == 0 || written > text.len() {
            return false;
        }
        text = &text[written..];
    }
    true
}
