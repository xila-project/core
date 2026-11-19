//! Model-based tests of the heap: a brute-force first-fit oracle for allocation, a model of the
//! free memory (everything that is not a live block) compared with the hole list after every
//! operation, and the exact result expected from every resize.

use super::*;
use alloc::vec::Vec;

const WORD: usize = size_of::<usize>();

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

#[repr(C, align(4096))]
struct Backing<const S: usize>([u8; S]);

fn leak<const S: usize>() -> &'static mut Backing<S> {
    alloc::boxed::Box::leak(alloc::boxed::Box::new(Backing([0xA5; S])))
}

fn holes(heap: &Heap) -> Vec<(usize, usize)> {
    let mut holes = Vec::new();
    let mut cursor = heap.first;
    while let Some(hole) = cursor {
        let Hole { size, next } = unsafe { ptr::read(hole.as_ptr()) };
        holes.push((hole.as_ptr().addr(), size));
        cursor = next;
    }
    holes
}

/// Independent oracle: lowest start in `[hole, hole + size)` that a block of `size` bytes
/// aligned to `align` can take, on the heap's granularity, without leaving a gap smaller than a
/// hole on either side.
fn brute_fit(hole: usize, hole_size: usize, size: usize, align: usize) -> Option<usize> {
    let end = hole + hole_size;
    let mut start = hole;
    while start + size <= end {
        let front = start - hole;
        let back = end - start - size;
        if start.is_multiple_of(align)
            && (front == 0 || front >= MIN_BLOCK)
            && (back == 0 || back >= MIN_BLOCK)
        {
            return Some(start);
        }
        start += BLOCK_ALIGN;
    }
    None
}

#[derive(Clone)]
struct Block {
    pointer: NonNull<u8>,
    layout: Layout,
    seed: u8,
}

fn stamp(block: &Block) {
    for i in 0..block.layout.size() {
        unsafe {
            block
                .pointer
                .as_ptr()
                .add(i)
                .write(block.seed.wrapping_add(i as u8))
        };
    }
}

fn check_stamp(block: &Block, length: usize) {
    for i in 0..length {
        assert_eq!(
            unsafe { block.pointer.as_ptr().add(i).read() },
            block.seed.wrapping_add(i as u8),
            "byte {i} of block {:#x} changed",
            block.pointer.addr()
        );
    }
}

/// Free memory must be exactly the complement of the live blocks.
fn expected_holes(heap: &Heap, blocks: &[Block]) -> Vec<(usize, usize)> {
    let mut taken: Vec<(usize, usize)> = blocks
        .iter()
        .map(|block| (block.pointer.addr().get(), footprint(block.layout).unwrap()))
        .collect();
    taken.sort();
    let mut expected = Vec::new();
    let mut cursor = heap.bottom().addr();
    for (address, size) in taken {
        assert!(address >= cursor, "blocks overlap at {address:#x}");
        if address > cursor {
            expected.push((cursor, address - cursor));
        }
        cursor = address + size;
    }
    assert!(cursor <= heap.top().addr(), "block past the top");
    if cursor < heap.top().addr() {
        expected.push((cursor, heap.top().addr() - cursor));
    }
    expected
}

fn check(heap: &Heap, blocks: &[Block]) {
    heap.validate();
    assert_eq!(holes(heap), expected_holes(heap, blocks));
    let used: usize = blocks
        .iter()
        .map(|block| footprint(block.layout).unwrap())
        .sum();
    assert_eq!(heap.used(), used);
    assert_eq!(heap.free(), heap.size() - used);
    for block in blocks {
        check_stamp(block, block.layout.size());
    }
}

fn random_layout(random: &mut Rng, heap_size: usize) -> Layout {
    let size = match random.below(8) {
        0 => random.below(2 * WORD + 1),
        1..=5 => random.below(200) + 1,
        6 => random.below(heap_size / 2) + 1,
        _ => random.below(heap_size + 64) + 1,
    };
    let align = 1usize << random.below(9);
    Layout::from_size_align(size, align).unwrap()
}

#[derive(Default, Debug)]
struct Counts {
    allocations: usize,
    allocation_failures: usize,
    shrinks: usize,
    grown: usize,
    refused_growths: usize,
}

/// Runs a random sequence against the model.
fn model_run<const S: usize>(seed: u64, operations: usize, offset: usize, counts: &mut Counts) {
    let backing = leak::<S>();
    let mut heap = Heap::empty();
    unsafe {
        heap.init(
            backing.0.as_mut_ptr().add(offset),
            S - offset - (seed as usize % 7),
        )
    };
    let heap_size = heap.size();

    let mut random = Rng(seed);
    let mut blocks: Vec<Block> = Vec::new();
    let mut next_seed = 0u8;

    for _ in 0..operations {
        match random.below(10) {
            0..=3 => {
                let layout = random_layout(&mut random, heap_size);
                let size = footprint(layout).unwrap();
                // First-fit oracle over the model's holes.
                let oracle = holes(&heap)
                    .into_iter()
                    .find_map(|(hole, hole_size)| brute_fit(hole, hole_size, size, layout.align()));
                let result = heap.allocate(layout);
                assert_eq!(
                    result.map(|pointer| pointer.addr().get()),
                    oracle,
                    "first fit disagrees for {layout:?}"
                );
                if let Some(pointer) = result {
                    counts.allocations += 1;
                    next_seed = next_seed.wrapping_add(31);
                    let block = Block {
                        pointer,
                        layout,
                        seed: next_seed,
                    };
                    stamp(&block);
                    blocks.push(block);
                } else {
                    counts.allocation_failures += 1;
                }
            }
            4..=5 if !blocks.is_empty() => {
                let block = blocks.swap_remove(random.below(blocks.len()));
                // The alignment of the layout may change between allocation and deallocation.
                let layout = Layout::from_size_align(block.layout.size(), 1).unwrap();
                unsafe { heap.deallocate(block.pointer, layout) };
            }
            _ if !blocks.is_empty() => {
                let index = random.below(blocks.len());
                let block = blocks[index].clone();
                let old = footprint(block.layout).unwrap();
                let new_size = match random.below(5) {
                    0 => block.layout.size().saturating_sub(WORD),
                    1 => block.layout.size() + (random.below(6) + 1) * WORD,
                    2 => block
                        .layout
                        .size()
                        .saturating_sub((random.below(6) + 1) * WORD),
                    3 => random.below(heap_size / 2) + 1,
                    _ => random.below(3 * WORD) + 1,
                }
                .max(1);
                let new_layout = Layout::from_size_align(new_size, block.layout.align()).unwrap();
                let new = footprint(new_layout).unwrap();
                let end = block.pointer.addr().get() + old;
                let next_hole = holes(&heap).into_iter().find(|(hole, _)| *hole == end);
                let before = holes(&heap);
                let used = heap.used();

                // A shrink always works; a growth works exactly when the hole right after the
                // block is large enough.
                let expected = if new <= old || next_hole.is_some_and(|(_, size)| size >= new - old)
                {
                    Resize::Done
                } else {
                    Resize::Impossible
                };

                let result =
                    unsafe { heap.resize_in_place(block.pointer, block.layout, new_layout) };
                assert_eq!(result, expected, "{:?} -> {:?}", block.layout, new_layout);
                match result {
                    Resize::Done => {
                        if new < old {
                            counts.shrinks += 1;
                        } else if new > old {
                            counts.grown += 1;
                        }
                        check_stamp(&block, block.layout.size().min(new_size));
                        next_seed = next_seed.wrapping_add(31);
                        blocks[index] = Block {
                            pointer: block.pointer,
                            layout: new_layout,
                            seed: next_seed,
                        };
                        stamp(&blocks[index]);
                    }
                    Resize::Impossible => {
                        counts.refused_growths += 1;
                        assert_eq!((holes(&heap), heap.used()), (before, used), "heap changed");
                    }
                }
            }
            _ => {}
        }
        check(&heap, &blocks);
    }

    for block in blocks.drain(..) {
        unsafe { heap.deallocate(block.pointer, block.layout) };
    }
    check(&heap, &[]);
    assert_eq!(holes(&heap), [(heap.bottom().addr(), heap.size())]);
}

#[test]
fn model_and_first_fit_oracle() {
    let operations = if cfg!(miri) { 300 } else { 4000 };
    // About 4 s in release mode, 4 s in debug mode.
    let seeds = if cfg!(miri) {
        2
    } else if cfg!(debug_assertions) {
        16
    } else {
        150
    };
    let mut counts = Counts::default();
    for seed in 1..=seeds {
        let seed = 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(seed);
        model_run::<1024>(seed, operations, 0, &mut counts);
        model_run::<8192>(seed, operations, 3, &mut counts);
    }
    // Every path was reached.
    assert!(
        counts.allocations > 0 && counts.allocation_failures > 0,
        "{counts:?}"
    );
    assert!(
        counts.shrinks > 0 && counts.grown > 0 && counts.refused_growths > 0,
        "{counts:?}"
    );
}

#[test]
fn extreme_layouts_do_not_panic() {
    let backing = leak::<1024>();
    let mut heap = Heap::empty();
    unsafe { heap.init(backing.0.as_mut_ptr(), 1024) };
    for layout in [
        Layout::from_size_align(isize::MAX as usize, 1).unwrap(),
        Layout::from_size_align(isize::MAX as usize - 4095, 4096).unwrap(),
        Layout::from_size_align(1, 1 << (usize::BITS - 2)).unwrap(),
        Layout::from_size_align(1025, 1).unwrap(),
    ] {
        assert!(heap.allocate(layout).is_none(), "{layout:?}");
    }
    // Exactly the whole heap, then exhaustion.
    let all = heap
        .allocate(Layout::from_size_align(1024, 1).unwrap())
        .unwrap();
    assert!(
        heap.allocate(Layout::from_size_align(1, 1).unwrap())
            .is_none()
    );
    assert_eq!(heap.free(), 0);
    unsafe { heap.deallocate(all, Layout::from_size_align(1024, 1).unwrap()) };
    heap.validate();
    // Smallest heap.
    let mut tiny = Heap::empty();
    unsafe { tiny.init(backing.0.as_mut_ptr(), MIN_BLOCK) };
    let block = tiny
        .allocate(Layout::from_size_align(0, 1).unwrap())
        .unwrap();
    assert_eq!(tiny.free(), 0);
    unsafe { tiny.deallocate(block, Layout::from_size_align(0, 1).unwrap()) };
    tiny.validate();
}

/// A caller may hold a pointer that only covers its block (for example one derived from a
/// `Box<[u8]>`). Freeing and resizing through it must not need more provenance than that.
#[test]
fn narrowed_pointers_can_resize_and_free() {
    let backing = leak::<1024>();
    let mut heap = Heap::empty();
    unsafe { heap.init(backing.0.as_mut_ptr(), 1024) };
    let narrow = |pointer: NonNull<u8>, length: usize| {
        let slice = unsafe { core::slice::from_raw_parts_mut(pointer.as_ptr(), length) };
        NonNull::from(slice).cast::<u8>()
    };

    let small = Layout::from_size_align(32, 8).unwrap();
    let large = Layout::from_size_align(96, 8).unwrap();
    let block = heap.allocate(small).unwrap();
    // Growing writes a hole header past the block, shrinking writes one inside the old block.
    assert_eq!(
        unsafe { heap.resize_in_place(narrow(block, 32), small, large) },
        Resize::Done
    );
    assert_eq!(
        unsafe { heap.resize_in_place(narrow(block, 96), large, small) },
        Resize::Done
    );
    unsafe { heap.deallocate(narrow(block, 32), small) };
    heap.validate();
    assert_eq!(heap.used(), 0);
    assert_eq!(holes(&heap), [(heap.bottom().addr(), heap.size())]);
}
