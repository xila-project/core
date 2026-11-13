#![no_std]
#![no_main]
use core::{alloc::Layout, panic::PanicInfo};
#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop()
    }
}
static mut POOL: [u128; 4096] = [0; 4096];
#[cfg(feature = "first-fit")]
type Heap = linked_list_allocator::Heap;
#[cfg(not(feature = "first-fit"))]
type Heap = rlsf::Tlsf<'static, usize, u32, 16, 8>;

#[unsafe(no_mangle)]
#[inline(never)]
unsafe fn allocate(heap: &mut Heap, layout: Layout) -> Option<core::ptr::NonNull<u8>> {
    #[cfg(feature = "first-fit")]
    {
        heap.allocate_first_fit(layout).ok()
    }
    #[cfg(not(feature = "first-fit"))]
    {
        heap.allocate(layout)
    }
}
#[unsafe(no_mangle)]
#[inline(never)]
unsafe fn deallocate(heap: &mut Heap, p: core::ptr::NonNull<u8>, layout: Layout) {
    #[cfg(feature = "first-fit")]
    unsafe {
        heap.deallocate(p, layout);
    }
    #[cfg(not(feature = "first-fit"))]
    unsafe {
        heap.deallocate(p, layout.align());
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    #[cfg(feature = "first-fit")]
    let mut heap = linked_list_allocator::Heap::empty();
    #[cfg(not(feature = "first-fit"))]
    let mut heap = rlsf::Tlsf::<'_, usize, u32, 16, 8>::new();
    unsafe {
        #[cfg(feature = "first-fit")]
        heap.init((&raw mut POOL).cast(), 65536);
        #[cfg(not(feature = "first-fit"))]
        heap.insert_free_block_ptr(core::ptr::NonNull::new_unchecked(
            core::ptr::slice_from_raw_parts_mut((&raw mut POOL).cast(), 65536),
        ));
        loop {
            let size = core::ptr::read_volatile((&raw const POOL).cast::<usize>()) % 256 + 16;
            let layout = Layout::from_size_align_unchecked(size, 8);
            if let Some(p) = allocate(&mut heap, layout) {
                core::ptr::write_volatile(p.as_ptr(), 42);
                deallocate(&mut heap, p, layout);
            }
        }
    }
}
