use crate::Status;
use alloc::alloc::{alloc, dealloc, realloc};
use core::{alloc::Layout, ffi::c_void, mem::size_of, ptr};

// The doc comment appears on both cfg branches: cbindgen copies it per branch.
/// Alignment guaranteed for every allocation, including zero-length requests.
#[cfg(target_arch = "arm")]
pub const XILA_HEAP_ALIGNMENT: usize = 8;
/// Alignment guaranteed for every allocation, including zero-length requests.
#[cfg(not(target_arch = "arm"))]
pub const XILA_HEAP_ALIGNMENT: usize = 16;

/// In-band block header: a doubly linked list keeps free and resize constant
/// time, and the tag identifies the owning heap.
struct Block {
    previous: *mut Block,
    next: *mut Block,
    size: usize,
    // Heap key XOR block address. Zero once the block is freed; a live tag is
    // never zero because the key is odd and block addresses are even.
    tag: usize,
}

const HEADER_SIZE: usize = size_of::<Block>().next_multiple_of(XILA_HEAP_ALIGNMENT);
// Four words must stay the whole per-block cost on 32-bit targets.
#[cfg(target_pointer_width = "32")]
const _: () = assert!(HEADER_SIZE == 16);

fn layout(size: usize) -> Result<Layout, Status> {
    let total = HEADER_SIZE.checked_add(size).ok_or(Status::OutOfMemory)?;
    Layout::from_size_align(total, XILA_HEAP_ALIGNMENT).map_err(|_| Status::OutOfMemory)
}

const KEY_SALT: usize = 0x5A17_A11C;
// Fibonacci hashing constant. Multiplying spreads nearby addresses apart, so
// heaps whose struct and first block sit at the same distance get different keys.
#[cfg(target_pointer_width = "32")]
const KEY_MULTIPLIER: usize = 0x9E37_79B9;
#[cfg(target_pointer_width = "64")]
const KEY_MULTIPLIER: usize = 0x9E37_79B9_7F4A_7C15;

/// Opaque native heap capability. The process owner drops it after all native
/// execution ends. Keep ownership outside any stack that can be abandoned.
///
/// Native applications are trusted, so misuse detection is cheap rather than
/// hardened. Every block starts with a header whose tag is the heap's key XOR
/// the block address. Free and resize read the header that would precede the
/// pointer and reject it as `InvalidArgument` unless it is aligned and its tag
/// matches: an interior pointer or a double free (including one through a
/// pointer that a moving resize left stale) is caught, and so is a
/// foreign-heap pointer unless the two heaps' keys collide, which is
/// unlikely. Like a C allocator, this reads through the pointer, so it needs
/// readable memory below it. It cannot detect a freed block whose memory was
/// reused by a later allocation of this heap, which then identifies the new
/// block. Metadata lives in the block, so no operation allocates it and a
/// shrinking resize never fails because of it.
pub struct Heap {
    head: *mut Block,
    // Zero until the first allocation. Never zero afterwards.
    key: usize,
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
            key: 0,
        }
    }

    fn tag(&self, block: *mut Block) -> usize {
        self.key ^ block.addr()
    }

    /// Return the block that owns `pointer`, which must be non-null.
    ///
    /// Trusted native contract: this reads the header below `pointer` before
    /// it can know the pointer is valid.
    fn block(&self, pointer: *mut c_void) -> Result<*mut Block, Status> {
        // An empty heap owns nothing, so there is no reason to read memory.
        if self.head.is_null()
            || !pointer.addr().is_multiple_of(XILA_HEAP_ALIGNMENT)
            || pointer.addr() <= HEADER_SIZE
        {
            return Err(Status::InvalidArgument);
        }
        let block = pointer.wrapping_byte_sub(HEADER_SIZE).cast::<Block>();
        // SAFETY: Trusted native contract. The pointer comes from some
        // allocation, so the bytes below it are readable.
        if unsafe { (*block).tag } != self.tag(block) {
            return Err(Status::InvalidArgument);
        }
        Ok(block)
    }

    fn allocate(&mut self, size: usize) -> Result<*mut c_void, Status> {
        let layout = layout(size)?;
        // SAFETY: The layout is never zero-sized.
        let block = unsafe { alloc(layout).cast::<Block>() };
        if block.is_null() {
            return Err(Status::OutOfMemory);
        }
        if self.key == 0 {
            // Live blocks have distinct addresses, so mixing in the first one
            // keeps heaps that reuse an address apart. A plain XOR would mostly
            // depend on their distance, which is the same for every process.
            let mixed = ptr::from_mut(self).addr().wrapping_mul(KEY_MULTIPLIER) ^ block.addr();
            self.key = (mixed.wrapping_mul(KEY_MULTIPLIER) ^ KEY_SALT) | 1;
        }
        // SAFETY: The block is a fresh allocation with room for the header and
        // the head is null or a live block of this heap.
        unsafe {
            block.write(Block {
                previous: ptr::null_mut(),
                next: self.head,
                size,
                tag: self.tag(block),
            });
            self.update_links(ptr::null_mut(), block, self.head);
            Ok(block.cast::<u8>().add(HEADER_SIZE).cast())
        }
    }

    fn reallocate(&mut self, pointer: *mut c_void, size: usize) -> Result<*mut c_void, Status> {
        if size == 0 {
            self.free(pointer)?;
            return Ok(ptr::null_mut());
        }
        if pointer.is_null() {
            return self.allocate(size);
        }
        let old = self.block(pointer)?;
        let new_layout = layout(size)?;
        // SAFETY: The tag matched, so the header belongs to a live block of
        // this heap that was allocated with the stored size.
        let old_layout = unsafe { layout((*old).size) };
        let Ok(old_layout) = old_layout else {
            unreachable!();
        };
        // A moving resize frees the old block without this heap writing to it
        // again, so clear its tag first: a stale pointer must not match.
        // Volatile: the allocator may free the block, and a plain store just
        // before that could be removed as dead.
        // SAFETY: The tag matched, so the old block is live.
        unsafe { ptr::addr_of_mut!((*old).tag).write_volatile(0) };
        // SAFETY: The old layout is the one the block was allocated with.
        let block = unsafe { realloc(old.cast(), old_layout, new_layout.size()).cast::<Block>() };
        if block.is_null() {
            // The old block is untouched apart from its tag, restored here.
            // SAFETY: Realloc failed, so the old block is still live.
            unsafe { (*old).tag = self.tag(old) };
            return Err(Status::OutOfMemory);
        }
        // SAFETY: Realloc moved the header with the block and the neighbors
        // are live blocks of this heap that still point at the old address.
        unsafe {
            (*block).size = size;
            (*block).tag = self.tag(block);
            self.update_links((*block).previous, block, (*block).next);
            Ok(block.cast::<u8>().add(HEADER_SIZE).cast())
        }
    }

    fn free(&mut self, pointer: *mut c_void) -> Result<(), Status> {
        if pointer.is_null() {
            return Ok(());
        }
        let block = self.block(pointer)?;
        // SAFETY: The tag matched, so the block and its neighbors are live
        // blocks of this heap.
        unsafe {
            let Block {
                previous,
                next,
                size,
                ..
            } = block.read();
            if previous.is_null() {
                self.head = next;
            } else {
                (*previous).next = next;
            }
            if !next.is_null() {
                (*next).previous = previous;
            }
            // A second free of this pointer then fails the tag check. Volatile,
            // so the store before dealloc is never removed as dead.
            ptr::addr_of_mut!((*block).tag).write_volatile(0);
            let Ok(layout) = layout(size) else {
                unreachable!();
            };
            dealloc(block.cast(), layout);
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
            // SAFETY: The head is a live block of this heap.
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
/// allocates a new block. A pointer that this heap does not own, an interior
/// pointer or a freed block returns `InvalidArgument`, with the same trusted
/// native contract as [`xila_heap_free`].
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

/// Free a block owned by this heap. Null succeeds. A pointer that this heap
/// does not own, an interior pointer or a block that was already freed returns
/// `InvalidArgument`. This check reads the block header through the pointer, so
/// the pointer must at least address readable memory (the trusted native
/// contract; see [`Heap`]).
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

    // Stackful switches stacks with inline assembly, which Miri cannot run.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn owner_reclaims_heap_after_coroutine_cancellation() {
        let before = testing::MemoryManager::thread_allocations();
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
        // Exactly the two blocks: the heap allocates no metadata.
        assert_eq!(delta.allocations(), 2);
        assert_eq!(delta.allocated_bytes(), 123 + 45 + 2 * HEADER_SIZE);

        // Cancelling frees the suspended stack, which holds no owner of the heap's blocks:
        // the heap releases them when the owner drops it. Nothing is left.
        drop(coroutine);
        drop(heap);
        let delta = testing::MemoryManager::thread_allocations().since(before);
        assert_eq!(delta.net_bytes(), 0);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod regression_tests {
    use super::*;

    #[test]
    fn header_stays_within_the_per_block_budget() {
        // 16 B of metadata per block on 32-bit targets; two words per link
        // pair, the size and the tag on 64-bit ones.
        assert_eq!(size_of::<Block>(), 4 * size_of::<usize>());
        assert!(HEADER_SIZE >= size_of::<Block>());
        assert!(HEADER_SIZE < size_of::<Block>() + XILA_HEAP_ALIGNMENT);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(HEADER_SIZE, 16);
    }

    // Initialize the payload so that header probes through interior pointers
    // never read uninitialized bytes.
    unsafe fn allocate_zeroed(heap: &mut Heap, size: usize) -> *mut c_void {
        let mut pointer = ptr::null_mut();
        unsafe {
            assert_eq!(xila_heap_allocate(heap, size, &mut pointer), Status::Ok);
            pointer.cast::<u8>().write_bytes(0, size);
        }
        pointer
    }

    #[test]
    fn foreign_and_interior_pointers_are_rejected() {
        let mut a = Heap::new();
        let mut b = Heap::new();
        unsafe {
            let mut p = allocate_zeroed(&mut a, 64);
            let q = allocate_zeroed(&mut b, 64);
            // Foreign pointer: rejected, both heaps intact.
            assert_eq!(xila_heap_free(&mut a, q), Status::InvalidArgument);
            assert_eq!(
                xila_heap_reallocate(&mut a, q, 64, &mut p),
                Status::InvalidArgument
            );
            // Misaligned interior pointer: rejected without reading memory.
            assert_eq!(
                xila_heap_free(&mut a, p.cast::<u8>().add(1).cast()),
                Status::InvalidArgument
            );
            // An aligned interior address is rejected by its tag.
            let interior = p.cast::<u8>().add(XILA_HEAP_ALIGNMENT).cast();
            assert_eq!(xila_heap_free(&mut a, interior), Status::InvalidArgument);
            assert_eq!(
                xila_heap_reallocate(&mut a, interior, 64, &mut p),
                Status::InvalidArgument
            );
            // Nothing was disturbed.
            assert_eq!(xila_heap_free(&mut a, p), Status::Ok);
            assert_eq!(xila_heap_free(&mut b, q), Status::Ok);
        }
    }

    // Reads freed memory, which is the trusted native contract of the tag
    // check and undefined behavior as far as Miri is concerned.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn double_free_is_rejected() {
        let mut heap = Heap::new();
        unsafe {
            let first = allocate_zeroed(&mut heap, 32);
            let second = allocate_zeroed(&mut heap, 32);
            assert_eq!(xila_heap_free(&mut heap, first), Status::Ok);
            assert_eq!(xila_heap_free(&mut heap, first), Status::InvalidArgument);
            let mut out = ptr::null_mut();
            assert_eq!(
                xila_heap_reallocate(&mut heap, first, 64, &mut out),
                Status::InvalidArgument
            );
            assert!(out.is_null());
            assert_eq!(xila_heap_free(&mut heap, second), Status::Ok);
        }
    }

    #[test]
    fn empty_heap_rejects_without_reading() {
        let mut heap = Heap::new();
        // Not dereferenceable: an empty heap owns nothing.
        let wild = ptr::without_provenance_mut(0x1000_0000);
        assert_eq!(
            unsafe { xila_heap_free(&mut heap, wild) },
            Status::InvalidArgument
        );
    }

    // Miri aborts on the near-isize::MAX request instead of failing it.
    #[test]
    #[cfg_attr(miri, ignore)]
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

    // Walk the list and check every block against its neighbors.
    fn assert_consistent(heap: &Heap, expected: usize) {
        let mut count = 0;
        let mut previous = ptr::null_mut();
        let mut block = heap.head;
        while !block.is_null() {
            // SAFETY: Every linked block is live.
            unsafe {
                assert_eq!((*block).previous, previous);
                assert_eq!((*block).tag, heap.tag(block));
                previous = block;
                block = (*block).next;
            }
            count += 1;
        }
        assert_eq!(count, expected);
    }

    #[test]
    fn blocks_unlink_from_any_position_and_survive_resizes() {
        let mut heap = Heap::new();
        let mut blocks = [ptr::null_mut(); 6];
        for (index, block) in blocks.iter_mut().enumerate() {
            *block = heap
                .allocate(16 + index)
                .unwrap_or_else(|_| panic!("test allocation failed"));
            // SAFETY: The block has at least one byte.
            unsafe { block.cast::<u8>().write(index as u8) };
        }
        assert_consistent(&heap, 6);
        // Grow and shrink the newest, the oldest and a middle block. The
        // neighbors must follow a block that moves.
        for index in [5, 0, 3] {
            let grown = heap.reallocate(blocks[index], 4096);
            blocks[index] = grown.unwrap_or_else(|_| panic!("grow failed"));
            assert_consistent(&heap, 6);
            let shrunk = heap.reallocate(blocks[index], 1);
            blocks[index] = shrunk.unwrap_or_else(|_| panic!("shrink failed"));
            assert_consistent(&heap, 6);
            // SAFETY: The first byte survives both resizes.
            assert_eq!(unsafe { blocks[index].cast::<u8>().read() }, index as u8);
        }
        // Free the head, the tail, a middle block and then the rest.
        let mut live = 6;
        for index in [5, 0, 3, 1, 4, 2] {
            assert_eq!(heap.free(blocks[index]), Ok(()));
            live -= 1;
            assert_consistent(&heap, live);
        }
        assert!(heap.head.is_null());
    }

    // `free(p)` after `q = realloc(p, bigger)` moved the block is the most
    // common C double free. The stale header must not match any more.
    #[test]
    #[cfg_attr(miri, ignore = "reads the freed block's header")]
    fn stale_pointer_after_moving_reallocate_is_rejected() {
        let mut heap = Heap::new();
        let mut moves = 0;
        for _ in 0..16 {
            let keep = heap
                .allocate(32)
                .unwrap_or_else(|_| panic!("allocation failed"));
            let old = heap
                .allocate(32)
                .unwrap_or_else(|_| panic!("allocation failed"));
            let new = heap
                .reallocate(old, 1 << 20)
                .unwrap_or_else(|_| panic!("grow failed"));
            if new != old {
                moves += 1;
                assert_eq!(heap.free(old), Err(Status::InvalidArgument));
            }
            assert_eq!(heap.free(new), Ok(()));
            assert_eq!(heap.free(keep), Ok(()));
        }
        // Growing a small block to 1 MiB must move it at least once, or this
        // test checks nothing.
        assert!(moves > 0);
    }

    // Process owners laid out one after another put each heap at the same
    // distance from its first block. Their keys must still differ.
    #[test]
    fn heaps_at_equal_distances_get_distinct_keys() {
        extern crate std;
        use std::{boxed::Box, vec::Vec};
        struct Process {
            _state: [u8; 48],
            heap: Heap,
        }
        let mut processes = Vec::new();
        let mut firsts = Vec::new();
        for _ in 0..64 {
            let mut process = Box::new(Process {
                _state: [0; 48],
                heap: Heap::new(),
            });
            let first = process.heap.allocate(24);
            firsts.push(first.unwrap_or_else(|_| panic!("allocation failed")));
            processes.push(process);
        }
        let mut keys: Vec<usize> = processes.iter().map(|process| process.heap.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), processes.len());
        for (process, first) in processes.iter_mut().zip(firsts) {
            assert_eq!(process.heap.free(first), Ok(()));
        }
    }
}
