#![no_std]
#![no_main]

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

/// Called by the entry shim with validated, host-owned startup data.
unsafe fn main(startup: *const XilaStartup) -> i32 {
    let api = unsafe { &*runtime::api() };
    let mut now = 0;
    if unsafe { (api.time_monotonic)(&mut now) } != Status::Ok {
        return 1;
    }

    // Exercise the optional process allocator, including a layout more aligned
    // than the host's heap. This example needs neither the alloc crate nor libc.
    let layout = Layout::from_size_align(32, 32).unwrap();
    let memory = unsafe { HEAP.alloc(layout) };
    if memory.is_null() {
        return 2;
    }
    let aligned = memory.addr().is_multiple_of(layout.align());
    unsafe { HEAP.dealloc(memory, layout) };
    if !aligned {
        return 2;
    }

    // The host test passes one argument to exercise the panic handler.
    if unsafe { (*startup).argument_count } != 0 {
        panic!("requested by the host test");
    }

    let output = unsafe { (*startup).standard_output };
    let mut text: &[u8] = b"Hello from Rust on Xila!\n";
    while !text.is_empty() {
        let mut written = 0;
        let result =
            unsafe { (api.file_write)(output, text.as_ptr().cast(), text.len(), &mut written) };
        if result != Status::Ok || written == 0 || written > text.len() {
            return 3;
        }
        text = &text[written..];
    }
    73
}
