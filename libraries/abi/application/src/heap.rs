use crate::Status;
use alloc::alloc::{alloc, dealloc, realloc};
use core::{alloc::Layout, ffi::c_void, mem::size_of, ptr, ptr::NonNull};
use hashbrown::HashTable;

/// Alignment guaranteed for every allocation, including zero-length requests.
#[cfg(target_arch = "arm")]
pub const XILA_HEAP_ALIGNMENT: usize = 8;
#[cfg(not(target_arch = "arm"))]
pub const XILA_HEAP_ALIGNMENT: usize = 16;

struct Block {
    previous: *mut Block,
    next: *mut Block,
    // Use the extra header word on 32-bit targets with 16-byte alignment.
    #[cfg(all(target_pointer_width = "32", not(target_arch = "arm")))]
    size: usize,
}

const HEADER_SIZE: usize = size_of::<Block>().next_multiple_of(XILA_HEAP_ALIGNMENT);
const _: () = assert!(HEADER_SIZE == XILA_HEAP_ALIGNMENT);

fn layout(size: usize) -> Result<Layout, Status> {
    let total = HEADER_SIZE.checked_add(size).ok_or(Status::OutOfMemory)?;
    Layout::from_size_align(total, XILA_HEAP_ALIGNMENT).map_err(|_| Status::OutOfMemory)
}

#[derive(Clone, Copy)]
struct Allocation {
    block: NonNull<Block>,
    // On Arm and 64-bit targets both links fill the header.
    #[cfg(any(target_arch = "arm", not(target_pointer_width = "32")))]
    size: usize,
}

impl Allocation {
    fn payload(self) -> *mut c_void {
        // SAFETY: Every indexed block has at least one payload byte.
        unsafe { self.block.as_ptr().cast::<u8>().add(HEADER_SIZE).cast() }
    }

    fn hash(&self) -> u64 {
        address_hash(self.block.addr().get())
    }

    // SAFETY: The block must still be live.
    unsafe fn layout(self) -> Layout {
        #[cfg(all(target_pointer_width = "32", not(target_arch = "arm")))]
        let size = unsafe { self.block.as_ref().size };
        #[cfg(any(target_arch = "arm", not(target_pointer_width = "32")))]
        let size = self.size;
        let Ok(layout) = layout(size) else {
            unreachable!();
        };
        layout
    }
}

fn address_hash(address: usize) -> u64 {
    // Drop the alignment bits and mix both ends of the address for SwissTable.
    let address = address / XILA_HEAP_ALIGNMENT;
    (address ^ (address >> 16)).wrapping_mul(0x9E37_79B9) as u64
}

/// Opaque native heap capability. The process owner drops it after all native
/// execution ends. Keep ownership outside any stack that can be abandoned.
pub struct Heap {
    head: *mut Block,
    // Validate the exact address before accessing memory. An in-band owner tag
    // would require reading through possibly freed or foreign pointers.
    blocks: HashTable<Allocation>,
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
            blocks: HashTable::new(),
        }
    }

    fn reserve_slot(&mut self) -> Result<(), Status> {
        self.blocks
            .try_reserve(1, Allocation::hash)
            .map_err(|_| Status::OutOfMemory)
    }

    fn allocate(&mut self, size: usize) -> Result<*mut c_void, Status> {
        let layout = layout(size)?;
        self.reserve_slot()?;
        let block =
            NonNull::new(unsafe { alloc(layout).cast::<Block>() }).ok_or(Status::OutOfMemory)?;
        unsafe {
            block.as_ptr().write(Block {
                previous: ptr::null_mut(),
                next: self.head,
                #[cfg(all(target_pointer_width = "32", not(target_arch = "arm")))]
                size,
            });
            self.update_links(ptr::null_mut(), block.as_ptr(), self.head);
        }
        Ok(self.record(block, size))
    }

    fn record(&mut self, block: NonNull<Block>, size: usize) -> *mut c_void {
        #[cfg(all(target_pointer_width = "32", not(target_arch = "arm")))]
        let _ = size;
        let allocation = Allocation {
            block,
            #[cfg(any(target_arch = "arm", not(target_pointer_width = "32")))]
            size,
        };
        self.blocks
            .insert_unique(allocation.hash(), allocation, Allocation::hash);
        allocation.payload()
    }

    fn reallocate(&mut self, pointer: *mut c_void, size: usize) -> Result<*mut c_void, Status> {
        if size == 0 {
            self.free(pointer)?;
            return Ok(ptr::null_mut());
        }
        if pointer.is_null() {
            return self.allocate(size);
        }
        let address = pointer.addr().wrapping_sub(HEADER_SIZE);
        let hash = address_hash(address);
        let matches = |entry: &Allocation| entry.block.addr().get() == address;
        let old = self
            .blocks
            .find(hash, matches)
            .copied()
            .ok_or(Status::InvalidArgument)?;
        let new_layout = layout(size)?;
        // Reserve before realloc can invalidate the old address. Moving the
        // entry may leave a tombstone and require a fresh bucket.
        self.reserve_slot()?;
        let block = NonNull::new(unsafe {
            realloc(old.block.as_ptr().cast(), old.layout(), new_layout.size()).cast::<Block>()
        })
        .ok_or(Status::OutOfMemory)?;
        let Ok(entry) = self.blocks.find_entry(hash, matches) else {
            unreachable!();
        };
        entry.remove();
        unsafe {
            let header = &mut *block.as_ptr();
            #[cfg(all(target_pointer_width = "32", not(target_arch = "arm")))]
            {
                header.size = size;
            }
            self.update_links(header.previous, block.as_ptr(), header.next);
        }
        Ok(self.record(block, size))
    }

    fn free(&mut self, pointer: *mut c_void) -> Result<(), Status> {
        if pointer.is_null() {
            return Ok(());
        }
        let address = pointer.addr().wrapping_sub(HEADER_SIZE);
        let Ok(entry) = self.blocks.find_entry(address_hash(address), |entry| {
            entry.block.addr().get() == address
        }) else {
            return Err(Status::InvalidArgument);
        };
        let (allocation, _) = entry.remove();
        unsafe {
            let block = allocation.block.as_ref();
            if block.previous.is_null() {
                self.head = block.next;
            } else {
                (*block.previous).next = block.next;
            }
            if !block.next.is_null() {
                (*block.next).previous = block.previous;
            }
            dealloc(allocation.block.as_ptr().cast(), allocation.layout());
        }
        Ok(())
    }

    // SAFETY: Non-null neighbors must be live, distinct blocks in this heap.
    unsafe fn update_links(&mut self, previous: *mut Block, block: *mut Block, next: *mut Block) {
        unsafe {
            if previous.is_null() {
                self.head = block;
            } else {
                (*previous).next = block;
            }
            if !next.is_null() {
                (*next).previous = block;
            }
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        while !self.head.is_null() {
            // SAFETY: The head is a live, indexed allocation.
            let pointer = unsafe { self.head.cast::<u8>().add(HEADER_SIZE).cast() };
            let Ok(()) = self.free(pointer) else {
                unreachable!();
            };
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
        assert_eq!(delta.allocations(), 3);
        assert!(delta.allocated_bytes() > 123 + 45 + 2 * HEADER_SIZE);

        // Stackful retains the suspended stack. Check only the allocations
        // made after entering it, which belong to the external heap.
        drop(coroutine);
        drop(heap);
        let delta = testing::MemoryManager::thread_allocations().since(before_resume);
        assert_eq!(delta.net_bytes(), 0);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod regression_tests {
    use super::*;

    #[test]
    fn double_free_and_foreign_pointer_are_rejected() {
        let mut a = Heap::new();
        let mut b = Heap::new();
        unsafe {
            let mut p = ptr::null_mut();
            let mut q = ptr::null_mut();
            assert_eq!(xila_heap_allocate(&mut a, 32, &mut p), Status::Ok);
            assert_eq!(xila_heap_allocate(&mut b, 32, &mut q), Status::Ok);
            // Foreign pointer: rejected, both heaps intact.
            assert_eq!(xila_heap_free(&mut a, q), Status::InvalidArgument);
            assert_eq!(
                xila_heap_reallocate(&mut a, q, 64, &mut p),
                Status::InvalidArgument
            );
            // Interior pointer: rejected.
            assert_eq!(
                xila_heap_free(&mut a, p.cast::<u8>().add(1).cast()),
                Status::InvalidArgument
            );
            // An aligned interior address must also be rejected before looking at any header.
            let interior = p.cast::<u8>().add(XILA_HEAP_ALIGNMENT).cast();
            assert_eq!(xila_heap_free(&mut a, interior), Status::InvalidArgument);
            assert_eq!(
                xila_heap_reallocate(&mut a, interior, 64, &mut p),
                Status::InvalidArgument
            );
            // Double free: second call rejected, no UB.
            assert_eq!(xila_heap_free(&mut a, p), Status::Ok);
            assert_eq!(xila_heap_free(&mut a, p), Status::InvalidArgument);
            assert_eq!(xila_heap_free(&mut b, q), Status::Ok);
        }
    }

    #[test]
    fn reallocate_failure_keeps_old_block_and_output() {
        let mut heap = Heap::new();
        unsafe {
            let mut p = ptr::null_mut();
            assert_eq!(xila_heap_allocate(&mut heap, 8, &mut p), Status::Ok);
            p.cast::<u64>().write(0x0123_4567_89AB_CDEF);
            let mut out = p;
            // Valid layout, but no allocator can satisfy it.
            let huge = isize::MAX as usize - 4096;
            let status = xila_heap_reallocate(&mut heap, p, huge, &mut out);
            assert_eq!(status, Status::OutOfMemory);
            assert_eq!(out, p);
            assert_eq!(p.cast::<u64>().read(), 0x0123_4567_89AB_CDEF);
            assert_eq!(
                xila_heap_allocate(&mut heap, usize::MAX, &mut out),
                Status::OutOfMemory
            );
            assert_eq!(out, p);
            assert_eq!(xila_heap_free(&mut heap, p), Status::Ok);
        }
    }

    #[test]
    fn free_cost_scales_linearly() {
        extern crate std;
        fn free_all(n: usize) -> std::time::Duration {
            let mut heap = Heap::new();
            let mut blocks = std::vec::Vec::with_capacity(n);
            for _ in 0..n {
                blocks.push(
                    heap.allocate(16)
                        .unwrap_or_else(|_| panic!("test allocation failed")),
                );
            }
            let start = std::time::Instant::now();
            for block in blocks {
                assert_eq!(heap.free(block), Ok(()));
            }
            start.elapsed()
        }

        // Oldest-first is the worst case for a list scan. Take the fastest of
        // three runs at each size to reduce the effect of scheduler preemption.
        let small = (0..3).map(|_| free_all(4096)).min().unwrap_or_default();
        let large = (0..3).map(|_| free_all(16384)).min().unwrap_or_default();
        std::println!("oldest-first free: 4096={small:?}, 16384={large:?}");
        assert!(large < 10 * small, "expected near-linear scaling");
    }
}
