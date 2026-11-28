use abi_application::XilaLifecycle;
use core::{ops::Range, ptr, ptr::NonNull};

#[derive(Clone, Copy)]
struct Array {
    start: *const usize,
    count: usize,
}

impl Array {
    unsafe fn call(self, index: usize) {
        // SAFETY: the loader verified this slot and its relocated code address.
        let address = unsafe { self.start.add(index).read_unaligned() };
        let function: unsafe extern "C" fn() = unsafe { core::mem::transmute(address) };
        unsafe { function() };
    }
}

#[repr(C)]
pub(crate) struct Lifecycle {
    interface: XilaLifecycle,
    arrays: [Array; 3],
}

impl Lifecycle {
    pub(crate) fn new(base: NonNull<u8>, ranges: &[Range<usize>; 3]) -> Self {
        Self {
            interface: XilaLifecycle { run: run_arrays },
            arrays: core::array::from_fn(|index| {
                let range = &ranges[index];
                Array {
                    start: base.as_ptr().wrapping_add(range.start).cast(),
                    count: range.len() / size_of::<usize>(),
                }
            }),
        }
    }

    pub(crate) fn interface(&self) -> *const XilaLifecycle {
        if self.arrays.iter().all(|array| array.count == 0) {
            ptr::null()
        } else {
            &self.interface
        }
    }
}

unsafe extern "C" fn run_arrays(lifecycle: *const XilaLifecycle, finalize: bool) {
    // SAFETY: the callback lives at offset zero in the loader's lifecycle record.
    let [preinitialize, initialize, finalize_array] =
        unsafe { (*lifecycle.cast::<Lifecycle>()).arrays };
    if finalize {
        for index in (0..finalize_array.count).rev() {
            unsafe { finalize_array.call(index) };
        }
    } else {
        for array in [preinitialize, initialize] {
            for index in 0..array.count {
                unsafe { array.call(index) };
            }
        }
    }
}
