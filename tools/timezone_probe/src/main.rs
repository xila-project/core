#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "allocator-control")]
extern crate alloc;

/// Retained link/size entry. No allocator, startup runtime or peripherals supplied.
#[unsafe(no_mangle)]
pub extern "C" fn probe(seconds: i64, zone_index: u32) -> i64 {
    #[cfg(feature = "allocator-control")]
    core::hint::black_box(alloc::boxed::Box::new(core::hint::black_box(seconds)));
    timezone_probe::convert(seconds, zone_index)
}
