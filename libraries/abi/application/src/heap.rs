use crate::Status;
use alloc::alloc::{alloc, dealloc, realloc};
use core::{alloc::Layout, ffi::c_void, mem::size_of, ptr};

/// Alignment guaranteed for every allocation, including zero-length requests.
pub const XILA_HEAP_ALIGNMENT: usize = 16;

struct Block {
    next: *mut Block,
    size: usize,
}

const HEADER_SIZE: usize = size_of::<Block>().next_multiple_of(XILA_HEAP_ALIGNMENT);
const _: () = assert!(HEADER_SIZE == 16);

fn layout(size: usize) -> Result<Layout, Status> {
    let total = HEADER_SIZE
        .checked_add(size.max(1))
        .ok_or(Status::Overflow)?;
    Layout::from_size_align(total, XILA_HEAP_ALIGNMENT).map_err(|_| Status::Overflow)
}

fn payload(block: *mut Block) -> *mut c_void {
    // Only called for live blocks allocated with at least HEADER_SIZE + 1 bytes.
    unsafe { block.cast::<u8>().add(HEADER_SIZE).cast() }
}

/// Opaque native heap capability. The process owner drops it after all native
/// execution ends. Keep ownership outside any stack that can be abandoned.
pub struct Heap {
    head: *mut Block,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    pub const fn new() -> Self {
        Self {
            head: ptr::null_mut(),
        }
    }

    fn allocate(&mut self, size: usize) -> Result<*mut c_void, Status> {
        let layout = layout(size)?;
        let block = unsafe { alloc(layout).cast::<Block>() };
        if block.is_null() {
            return Err(Status::OutOfMemory);
        }
        unsafe {
            block.write(Block {
                next: self.head,
                size,
            })
        };
        self.head = block;
        Ok(payload(block))
    }

    fn link(&mut self, pointer: *mut c_void) -> Result<&mut *mut Block, Status> {
        let mut link = &mut self.head;
        while !link.is_null() {
            if payload(*link) == pointer {
                return Ok(link);
            }
            link = unsafe { &mut (**link).next };
        }
        Err(Status::InvalidArgument)
    }

    fn reallocate(&mut self, pointer: *mut c_void, size: usize) -> Result<*mut c_void, Status> {
        if size == 0 {
            self.free(pointer)?;
            return Ok(ptr::null_mut());
        }
        if pointer.is_null() {
            return self.allocate(size);
        }
        let new_layout = layout(size)?;
        let link = self.link(pointer)?;
        let old_layout = layout(unsafe { (**link).size }).unwrap();
        let block =
            unsafe { realloc((*link).cast(), old_layout, new_layout.size()).cast::<Block>() };
        if block.is_null() {
            return Err(Status::OutOfMemory);
        }
        unsafe { (*block).size = size };
        *link = block;
        Ok(payload(block))
    }

    fn free(&mut self, pointer: *mut c_void) -> Result<(), Status> {
        if !pointer.is_null() {
            Self::unlink(self.link(pointer)?);
        }
        Ok(())
    }

    fn unlink(link: &mut *mut Block) {
        let block = *link;
        unsafe {
            *link = (*block).next;
            dealloc(block.cast(), layout((*block).size).unwrap());
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        while !self.head.is_null() {
            Self::unlink(&mut self.head);
        }
    }
}

/// Allocate uninitialized bytes. A zero-size request reserves one byte.
/// # Safety
/// Heap must be live and exclusively accessed. Output must be aligned, writable
/// and disjoint from the heap and its allocations.
pub unsafe extern "C" fn xila_heap_allocate(
    heap: *mut Heap,
    size: usize,
    allocation: *mut *mut c_void,
) -> Status {
    unsafe { xila_heap_reallocate(heap, ptr::null_mut(), size.max(1), allocation) }
}

/// Resize a block, preserving its contents up to the smaller of the old and
/// new sizes. Failure leaves the old allocation and output unchanged. Size
/// zero frees the block and stores null. A null pointer with nonzero size
/// allocates a new block.
/// # Safety
/// Heap must be live and exclusively accessed. Output must be aligned, writable
/// and disjoint from the heap and its allocations.
pub unsafe extern "C" fn xila_heap_reallocate(
    heap: *mut Heap,
    pointer: *mut c_void,
    size: usize,
    allocation: *mut *mut c_void,
) -> Status {
    let Some(heap) = (unsafe { heap.as_mut() }) else {
        return Status::InvalidArgument;
    };
    if allocation.is_null() {
        return Status::InvalidArgument;
    }
    match heap.reallocate(pointer, size) {
        Ok(pointer) => {
            unsafe { allocation.write(pointer) };
            Status::Ok
        }
        Err(error) => error,
    }
}

/// Free a block owned by this heap. Null succeeds.
/// # Safety
/// Heap must be live and exclusively accessed.
pub unsafe extern "C" fn xila_heap_free(heap: *mut Heap, pointer: *mut c_void) -> Status {
    let Some(heap) = (unsafe { heap.as_mut() }) else {
        return Status::InvalidArgument;
    };
    heap.free(pointer)
        .map_or_else(|error| error, |()| Status::Ok)
}

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod tests {
    use super::*;

    #[test]
    fn owner_reclaims_heap_after_coroutine_cancellation() {
        let mut heap = Heap::new();
        let capability = &mut heap as *mut Heap;
        let before = testing::MemoryManager::thread_allocations();
        let mut coroutine = unsafe {
            stackful::Coroutine::new(stackful::Stack::new(64 * 1024), move || {
                let mut block = ptr::null_mut();
                assert_eq!(xila_heap_allocate(capability, 123, &mut block), Status::Ok);
                assert_eq!(xila_heap_allocate(capability, 45, &mut block), Status::Ok);
                stackful::suspend_on(core::future::pending::<()>());
                0
            })
        };
        let before_resume = testing::MemoryManager::thread_allocations();
        let mut context = core::task::Context::from_waker(core::task::Waker::noop());
        assert!(coroutine.resume(&mut context).is_pending());
        let delta = testing::MemoryManager::thread_allocations().since(before_resume);
        assert_eq!(delta.allocations(), 2);
        assert_eq!(delta.allocated_bytes(), 123 + 45 + 2 * HEADER_SIZE);

        drop(coroutine);
        drop(heap);
        let delta = testing::MemoryManager::thread_allocations().since(before);
        assert_eq!(delta.deallocated_bytes(), 123 + 45 + 2 * HEADER_SIZE);
    }
}
