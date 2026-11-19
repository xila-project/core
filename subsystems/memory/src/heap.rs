//! Address-ordered free-list heap that can resize a block in place.
//!
//! The block model is the one of `linked_list_allocator::Heap` (first fit, free holes stored inside
//! the free memory itself, no per-allocation header), so allocation, deallocation and statistics
//! behave the same. `linked_list_allocator` keeps its hole list private, which makes it impossible
//! to ask whether the memory right after a block is free. This module keeps the list reachable so
//! a block can grow into the following hole or give its tail back without moving.
//!
//! The RAM cost is lower than the previous heap: four words per region instead of six.
//!
//! # Block model
//!
//! * A block occupies `footprint(layout)` bytes: the layout size, raised to [`MIN_BLOCK`] and
//!   rounded up to [`BLOCK_ALIGN`]. Only the size matters, so the alignment of a layout can change
//!   between allocation and deallocation.
//! * Free holes are kept sorted by address and are never adjacent (they are merged on free).
//!   Every hole is at least [`MIN_BLOCK`] bytes, with a size and address multiple of
//!   [`BLOCK_ALIGN`].
//! * A region never holds bytes that are neither in a hole nor in a block, so
//!   `used + free == size` is exact.

use core::{
    alloc::Layout,
    mem::{align_of, size_of},
    ptr::{self, NonNull, null_mut},
};

/// Header of a free hole, stored at the start of the hole itself.
#[repr(C)]
struct Hole {
    size: usize,
    next: Option<NonNull<Hole>>,
}

/// Alignment and granularity of every block and hole.
const BLOCK_ALIGN: usize = align_of::<Hole>();

/// Smallest block, and smallest hole: a hole must be able to store its own header.
pub(crate) const MIN_BLOCK: usize = size_of::<Hole>();

/// Bytes a block of `layout` occupies in the heap.
fn footprint(layout: Layout) -> Option<usize> {
    layout
        .size()
        .max(MIN_BLOCK)
        .checked_next_multiple_of(BLOCK_ALIGN)
}

fn align_up(address: usize, align: usize) -> Option<usize> {
    Some(address.checked_add(align - 1)? & !(align - 1))
}

/// Outcome of [`Heap::resize_in_place`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resize {
    /// The block now has the requested size and kept its address.
    Done,
    /// The block shrinks by a single word and no hole follows it.
    ///
    /// A word is smaller than a hole and the heap has no header to record it, so releasing it
    /// would lose it for good. The caller either moves the block, which is exact, or keeps the
    /// block as it is, which leaves that word with the block as unreclaimable slack.
    OneWordTail,
    /// The block cannot be resized where it is.
    Impossible,
}

/// A heap over one memory region.
pub(crate) struct Heap {
    first: Option<NonNull<Hole>>,
    bottom: *mut u8,
    top: *mut u8,
    used: usize,
}

// The heap owns the memory it manages and is only reached through a critical section.
unsafe impl Send for Heap {}

impl Heap {
    /// Creates an empty heap that serves no allocation.
    pub(crate) const fn empty() -> Self {
        Self {
            first: None,
            bottom: null_mut(),
            top: null_mut(),
            used: 0,
        }
    }

    /// Hands `[start, start + size)` to the heap.
    ///
    /// The start is rounded up and the end rounded down to [`BLOCK_ALIGN`].
    ///
    /// # Safety
    ///
    /// The range must be valid, writable and unaliased for the rest of the program, and not shared
    /// with another heap. It must hold at least [`MIN_BLOCK`] bytes once aligned.
    pub(crate) unsafe fn init(&mut self, start: *mut u8, size: usize) {
        let skipped =
            align_up(start.addr(), BLOCK_ALIGN).expect("heap start overflows") - start.addr();
        let usable = size
            .checked_sub(skipped)
            .expect("heap smaller than its alignment")
            & !(BLOCK_ALIGN - 1);
        assert!(usable >= MIN_BLOCK, "heap smaller than a hole");

        // SAFETY: `skipped <= size`, so the aligned start stays in the caller's range.
        let bottom = unsafe { start.add(skipped) };
        let hole = bottom.cast::<Hole>();
        // SAFETY: `bottom` is aligned for `Hole` and has at least `MIN_BLOCK` bytes.
        unsafe {
            hole.write(Hole {
                size: usable,
                next: None,
            })
        };

        self.first = NonNull::new(hole);
        self.bottom = bottom;
        // SAFETY: `usable <= size - skipped`.
        self.top = unsafe { bottom.add(usable) };
        self.used = 0;
    }

    /// Aligned address of the first usable byte.
    pub(crate) fn bottom(&self) -> *mut u8 {
        self.bottom
    }

    /// Address one past the last usable byte.
    pub(crate) fn top(&self) -> *mut u8 {
        self.top
    }

    /// Usable bytes, free or not.
    pub(crate) fn size(&self) -> usize {
        self.top.addr() - self.bottom.addr()
    }

    /// Bytes held by live blocks.
    pub(crate) fn used(&self) -> usize {
        self.used
    }

    /// Bytes held by free holes.
    pub(crate) fn free(&self) -> usize {
        self.size() - self.used
    }

    /// Allocates a block with the first hole that fits.
    pub(crate) fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = footprint(layout)?;
        let align = layout.align();

        let mut link: *mut Option<NonNull<Hole>> = &mut self.first;
        // SAFETY: `link` always points to `self.first` or to the `next` field of a live hole, and
        // every hole header was written by this heap.
        unsafe {
            while let Some(hole) = *link {
                if let Some(block) = Self::carve(link, hole, size, align) {
                    self.used += size;
                    return Some(block);
                }
                link = &raw mut (*hole.as_ptr()).next;
            }
        }
        None
    }

    /// Takes a block of `size` bytes aligned to `align` out of `hole`, if it fits.
    ///
    /// `link` is the pointer to `hole` in the previous hole (or the head). On success the hole is
    /// replaced by the holes left before and after the block.
    ///
    /// # Safety
    ///
    /// `link` must hold `Some(hole)` and `hole` must be a live hole of this heap.
    unsafe fn carve(
        link: *mut Option<NonNull<Hole>>,
        hole: NonNull<Hole>,
        size: usize,
        align: usize,
    ) -> Option<NonNull<u8>> {
        let start = hole.as_ptr().cast::<u8>();
        // SAFETY: `hole` is a live hole.
        let Hole {
            size: hole_size,
            next,
        } = unsafe { ptr::read(hole.as_ptr()) };
        if hole_size < size {
            return None;
        }

        // Leading gap: none when the hole already fits the alignment, otherwise at least a whole
        // hole so the gap stays a valid hole.
        let address = start.addr();
        let front = if address.is_multiple_of(align) {
            0
        } else {
            align_up(address.checked_add(MIN_BLOCK)?, align)? - address
        };
        let end = front.checked_add(size)?;
        if end > hole_size {
            return None;
        }

        // Trailing gap: it must be empty or a valid hole, otherwise bytes would be lost.
        let back = hole_size - end;
        if back != 0 && back < MIN_BLOCK {
            return None;
        }

        // SAFETY: `front + size + back == hole_size`, so every pointer stays inside the hole.
        // The header values were read above, before any write can overwrite them.
        unsafe {
            let mut following = next;
            if back != 0 {
                let after = start.add(end).cast::<Hole>();
                after.write(Hole {
                    size: back,
                    next: following,
                });
                following = Some(NonNull::new_unchecked(after));
            }
            if front != 0 {
                hole.as_ptr().write(Hole {
                    size: front,
                    next: following,
                });
            } else {
                *link = following;
            }
            Some(NonNull::new_unchecked(start.add(front)))
        }
    }

    /// Frees a block.
    ///
    /// # Safety
    ///
    /// `pointer` must come from [`Self::allocate`] of this heap (possibly resized in place), and
    /// `layout` must have the size of the block (the alignment is ignored).
    pub(crate) unsafe fn deallocate(&mut self, pointer: NonNull<u8>, layout: Layout) {
        let size = footprint(layout).expect("layout was allocated");
        // SAFETY: forwarded from the caller; a whole block is at least `MIN_BLOCK`.
        let inserted = unsafe { self.release(pointer.as_ptr(), size) };
        debug_assert!(inserted);
        self.used -= size;
    }

    /// Changes the size of a live block without moving it.
    ///
    /// * Shrinking releases the tail, merged with the hole that follows it.
    /// * Growing succeeds when a hole starts right after the block, is large enough, and leaves
    ///   either nothing or a valid hole behind.
    ///
    /// The heap is left untouched unless the result is [`Resize::Done`].
    ///
    /// # Safety
    ///
    /// `pointer` must be a live block of `old_layout` and must be aligned for `new_layout`.
    pub(crate) unsafe fn resize_in_place(
        &mut self,
        pointer: NonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Resize {
        let (Some(old), Some(new)) = (footprint(old_layout), footprint(new_layout)) else {
            return Resize::Impossible;
        };

        if new == old {
            return Resize::Done;
        }
        if new < old {
            let tail = old - new;
            // SAFETY: the tail is inside the block, which the caller owns.
            return if unsafe { self.release(pointer.as_ptr().add(new), tail) } {
                self.used -= tail;
                Resize::Done
            } else {
                Resize::OneWordTail
            };
        }

        let extra = new - old;
        // SAFETY: the block lies inside the heap, so its end is at most `top`.
        let end = unsafe { pointer.as_ptr().add(old) };

        let mut link: *mut Option<NonNull<Hole>> = &mut self.first;
        // SAFETY: same list invariants as in `allocate`.
        unsafe {
            while let Some(hole) = *link {
                let address = hole.as_ptr().cast::<u8>().addr();
                if address == end.addr() {
                    let Hole { size, next } = ptr::read(hole.as_ptr());
                    match size.checked_sub(extra) {
                        Some(0) => *link = next,
                        Some(rest) if rest >= MIN_BLOCK => {
                            let moved = end.add(extra).cast::<Hole>();
                            moved.write(Hole { size: rest, next });
                            *link = Some(NonNull::new_unchecked(moved));
                        }
                        _ => return Resize::Impossible,
                    }
                    self.used += extra;
                    return Resize::Done;
                }
                if address > end.addr() {
                    return Resize::Impossible;
                }
                link = &raw mut (*hole.as_ptr()).next;
            }
        }
        Resize::Impossible
    }

    /// Turns `[start, start + size)` into free memory, merging it with its neighbours.
    ///
    /// A range smaller than [`MIN_BLOCK`] is only accepted when it touches the hole after it;
    /// otherwise nothing changes and `false` is returned.
    ///
    /// # Safety
    ///
    /// The range must be inside the heap, owned by the caller and not already free.
    unsafe fn release(&mut self, start: *mut u8, size: usize) -> bool {
        let mut link: *mut Option<NonNull<Hole>> = &mut self.first;
        let mut previous: Option<NonNull<Hole>> = None;

        // SAFETY: same list invariants as in `allocate`; holes are sorted by address.
        unsafe {
            while let Some(hole) = *link {
                if hole.as_ptr().cast::<u8>().addr() > start.addr() {
                    break;
                }
                previous = Some(hole);
                link = &raw mut (*hole.as_ptr()).next;
            }

            let mut size = size;
            let mut following = *link;
            if let Some(next) = following {
                let address = next.as_ptr().cast::<u8>().addr();
                debug_assert!(start.addr() + size <= address, "block overlaps a free hole");
                if start.addr() + size == address {
                    let Hole {
                        size: next_size,
                        next: after,
                    } = ptr::read(next.as_ptr());
                    size += next_size;
                    following = after;
                }
            }

            if let Some(previous) = previous {
                let end = previous.as_ptr().cast::<u8>().addr() + (*previous.as_ptr()).size;
                debug_assert!(end <= start.addr(), "block overlaps a free hole");
                if end == start.addr() {
                    (*previous.as_ptr()).size += size;
                    (*previous.as_ptr()).next = following;
                    return true;
                }
            }

            if size < MIN_BLOCK {
                return false;
            }
            let hole = start.cast::<Hole>();
            hole.write(Hole {
                size,
                next: following,
            });
            *link = Some(NonNull::new_unchecked(hole));
            true
        }
    }

    /// Walks the hole list and checks its invariants; test builds only.
    #[cfg(test)]
    pub(crate) fn validate(&self) {
        let mut previous_end = 0usize;
        let mut free = 0usize;
        let mut cursor = self.first;
        while let Some(hole) = cursor {
            // SAFETY: holes are live headers inside the heap.
            let Hole { size, next } = unsafe { ptr::read(hole.as_ptr()) };
            let address = hole.as_ptr().addr();
            assert!(address >= self.bottom.addr() && address + size <= self.top.addr());
            assert_eq!(address % BLOCK_ALIGN, 0, "hole address alignment");
            assert_eq!(size % BLOCK_ALIGN, 0, "hole size granularity");
            assert!(size >= MIN_BLOCK, "hole below the minimum size");
            assert!(address > previous_end, "holes unsorted or adjacent");
            previous_end = address + size;
            free += size;
            cursor = next;
        }
        assert_eq!(free + self.used, self.size());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C, align(64))]
    struct Backing([u8; 4096]);

    const WORD: usize = size_of::<usize>();

    fn heap() -> Heap {
        let backing = alloc::boxed::Box::leak(alloc::boxed::Box::new(Backing([0; 4096])));
        let mut heap = Heap::empty();
        unsafe { heap.init(backing.0.as_mut_ptr(), backing.0.len()) };
        heap
    }

    fn layout(size: usize) -> Layout {
        Layout::from_size_align(size, WORD).unwrap()
    }

    fn alloc_block(heap: &mut Heap, size: usize) -> NonNull<u8> {
        let block = heap.allocate(layout(size)).unwrap();
        heap.validate();
        block
    }

    /// Start address and size of every hole.
    fn holes(heap: &Heap) -> alloc::vec::Vec<(usize, usize)> {
        let mut holes = alloc::vec::Vec::new();
        let mut cursor = heap.first;
        while let Some(hole) = cursor {
            let hole = unsafe { hole.as_ref() };
            holes.push(((hole as *const Hole).addr(), hole.size));
            cursor = hole.next;
        }
        holes
    }

    #[test]
    fn init_aligns_the_range() {
        let backing = alloc::boxed::Box::leak(alloc::boxed::Box::new(Backing([0; 4096])));
        let start = unsafe { backing.0.as_mut_ptr().add(1) };
        let mut heap = Heap::empty();
        unsafe { heap.init(start, 1001) };
        assert_eq!(heap.bottom().addr() % BLOCK_ALIGN, 0);
        assert_eq!(heap.size() % BLOCK_ALIGN, 0);
        assert!(heap.size() <= 1001 && heap.size() > 1001 - 2 * BLOCK_ALIGN);
        assert_eq!(heap.free(), heap.size());
        heap.validate();
    }

    #[test]
    fn blocks_are_at_least_a_hole_and_freeing_merges_everything() {
        let mut heap = heap();
        let size = heap.size();
        let a = alloc_block(&mut heap, 1);
        let b = alloc_block(&mut heap, 100);
        let c = alloc_block(&mut heap, 9);
        assert_eq!(
            heap.used(),
            MIN_BLOCK + footprint(layout(100)).unwrap() + footprint(layout(9)).unwrap()
        );

        unsafe {
            heap.deallocate(b, layout(100));
            heap.validate();
            heap.deallocate(a, layout(1));
            heap.validate();
            heap.deallocate(c, layout(9));
        }
        heap.validate();
        assert_eq!(heap.used(), 0);
        assert_eq!(holes(&heap), [(heap.bottom().addr(), size)]);
    }

    #[test]
    fn alignment_gaps_stay_valid_holes() {
        let mut heap = heap();
        let first = alloc_block(&mut heap, 24);
        let aligned = heap
            .allocate(Layout::from_size_align(64, 64).unwrap())
            .unwrap();
        heap.validate();
        assert_eq!(aligned.addr().get() % 64, 0);
        unsafe {
            heap.deallocate(aligned, layout(64));
            heap.deallocate(first, layout(24));
        }
        heap.validate();
        assert_eq!(heap.used(), 0);
        assert_eq!(holes(&heap).len(), 1);
    }

    #[test]
    fn shrink_releases_the_tail_and_merges_it_with_the_next_hole() {
        let mut heap = heap();
        let block = alloc_block(&mut heap, 256);
        let guard = alloc_block(&mut heap, 32);
        let used = heap.used();

        // Next block busy: the tail becomes a hole of its own.
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(256), layout(128)) },
            Resize::Done
        );
        heap.validate();
        assert_eq!(heap.used(), used - 128);
        assert_eq!(holes(&heap)[0], (block.addr().get() + 128, 128));

        // Next hole adjacent: merged with it.
        unsafe { heap.deallocate(guard, layout(32)) };
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(128), layout(64)) },
            Resize::Done
        );
        heap.validate();
        assert_eq!(holes(&heap).len(), 1);
        assert_eq!(heap.used(), 64);
    }

    #[test]
    fn a_one_word_tail_is_only_taken_back_by_an_adjacent_hole() {
        let mut heap = heap();
        let block = alloc_block(&mut heap, 64);
        let guard = alloc_block(&mut heap, 32);
        let used = heap.used();

        // Busy neighbour: the word cannot be recorded, so the heap stays as it is.
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(64), layout(64 - WORD)) },
            Resize::OneWordTail
        );
        assert_eq!(heap.used(), used);
        heap.validate();

        // Free neighbour: the word joins the hole.
        unsafe { heap.deallocate(guard, layout(32)) };
        let used = heap.used();
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(64), layout(64 - WORD)) },
            Resize::Done
        );
        assert_eq!(heap.used(), used - WORD);
        assert_eq!(holes(&heap)[0].0, block.addr().get() + 64 - WORD);
        heap.validate();
    }

    #[test]
    fn grow_takes_the_hole_after_the_block() {
        let mut heap = heap();
        let block = alloc_block(&mut heap, 64);
        let guard = alloc_block(&mut heap, 64);
        let tail = alloc_block(&mut heap, 64);
        unsafe { heap.deallocate(guard, layout(64)) };

        // Part of the hole: the rest of it stays a hole.
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(64), layout(96)) },
            Resize::Done
        );
        heap.validate();
        assert!(holes(&heap).contains(&(block.addr().get() + 96, 32)));

        // Leaving less than a hole is refused and changes nothing.
        let used = heap.used();
        let before = holes(&heap);
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(96), layout(96 + 32 - WORD)) },
            Resize::Impossible
        );
        assert_eq!((heap.used(), holes(&heap)), (used, before));

        // The whole hole.
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(96), layout(128)) },
            Resize::Done
        );
        heap.validate();
        assert_eq!(heap.used(), 128 + 64);

        // A busy neighbour blocks growth.
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(128), layout(129)) },
            Resize::Impossible
        );
        unsafe {
            heap.deallocate(tail, layout(64));
            heap.deallocate(block, layout(128));
        }
        heap.validate();
        assert_eq!(heap.used(), 0);
    }

    #[test]
    fn a_block_cannot_grow_past_the_end_of_the_heap() {
        let mut heap = heap();
        let size = heap.size();
        let block = alloc_block(&mut heap, size - 64);
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(size - 64), layout(size + 8)) },
            Resize::Impossible
        );
        assert_eq!(
            unsafe { heap.resize_in_place(block, layout(size - 64), layout(size)) },
            Resize::Done
        );
        assert_eq!(heap.free(), 0);
        assert!(holes(&heap).is_empty());
        heap.validate();
    }
}
