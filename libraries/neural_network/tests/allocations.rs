#![cfg(feature = "cmsis-nn")]
//! Count all Rust heap allocations in the VFS driver path after open.
use file_system::{BaseOperations, Context, ControlCommand};
use neural_network::{COMPUTE, GET_INPUT_INFO};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Counting;
fn record() {
    ALLOCATIONS.with(|count| {
        if let Some(n) = count.get() {
            count.set(Some(n + 1));
        }
    });
}
// SAFETY: all operations forward their original pointer and layout to System.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
#[repr(align(16))]
struct Aligned<const N: usize>([u8; N]);
static MODEL: Aligned<{ include_bytes!("fixtures/classifier.tflite").len() }> =
    Aligned(*include_bytes!("fixtures/classifier.tflite"));

#[test]
fn vfs_driver_write_compute_read_allocates_nothing_after_open() {
    let device = neural_network::NeuralNetworkDevice::new(&MODEL.0, nanoglia::Target::Cpu).unwrap();
    let mut context = Context::new_empty();
    device.open(&mut context).unwrap();
    let mut info = neural_network::TensorInfo::default();
    device
        .control_sync(
            &mut context,
            GET_INPUT_INFO::IDENTIFIER,
            shared::AnyByLayout::from(&0u32),
            shared::AnyByLayout::from_mutable(&mut info),
        )
        .unwrap();
    let input = vec![0; info.byte_len as usize];
    let mut output = [0; 64];
    ALLOCATIONS.with(|count| count.set(Some(0)));
    for _ in 0..5 {
        assert_eq!(device.write_sync(&mut context, &input, 0), Ok(input.len()));
        device
            .control_sync(
                &mut context,
                COMPUTE::IDENTIFIER,
                shared::AnyByLayout::NONE,
                shared::AnyByLayout::NONE,
            )
            .unwrap();
        assert!(device.read_sync(&mut context, &mut output, 0).unwrap() > 0);
    }
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(allocations, 0);
    device.close(&mut context).unwrap();
}
