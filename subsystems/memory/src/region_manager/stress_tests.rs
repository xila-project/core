//! Region manager tests that need several regions or many operations: the property test of the
//! reallocation policy, the regression tests for the one-word shrink and for pointer provenance,
//! and a multi-threaded test.

extern crate std;

use super::*;
use alloc::vec::Vec;
use core::mem::size_of;

const WORD: usize = size_of::<usize>();

#[repr(C, align(4096))]
struct Backing<const S: usize>([MaybeUninit<u8>; S]);

fn leak<const S: usize>() -> &'static mut [MaybeUninit<u8>] {
    &mut alloc::boxed::Box::leak(alloc::boxed::Box::new(Backing::<S>(
        [const { MaybeUninit::uninit() }; S],
    )))
    .0
}

fn footprint(size: usize) -> usize {
    size.max(2 * WORD).next_multiple_of(2 * WORD)
}

fn validate<const N: usize>(manager: &RegionManager<N>) {
    critical_section::with(|cs| {
        for region in manager.inner.borrow_ref(cs).regions.iter() {
            if !region.is_empty() {
                region.heap.validate();
            }
        }
    });
}

fn largest_allocation<const N: usize>(manager: &RegionManager<N>) -> usize {
    let mut size = manager.get_total_size();
    while size > 0 {
        let layout = Layout::from_size_align(size, 1).unwrap();
        if let Some(pointer) = unsafe { manager.allocate(CapabilityFlags::empty(), layout) } {
            unsafe { manager.deallocate(pointer, layout) };
            return size;
        }
        size -= 2 * WORD;
    }
    0
}

/// A one-word shrink in a full region keeps the block where it is and nothing is stranded: once
/// everything is freed, `used` is zero and the region serves a block of its own size again. A
/// heap that could not record the word used to keep it as slack for good.
#[test]
fn a_one_word_shrink_in_a_full_region_leaks_nothing() {
    let manager = RegionManager::new([RegionDescriptor::new(CapabilityFlags::empty())], None);
    assert!(manager.init_region_from_slice(0, leak::<4096>()));
    let total = manager.get_total_size();
    assert_eq!(largest_allocation(&manager), total);

    let half = Layout::from_size_align(total / 2, WORD).unwrap();
    let rest = Layout::from_size_align(total - total / 2, WORD).unwrap();
    let a = unsafe { manager.allocate(CapabilityFlags::empty(), half) }.unwrap();
    let b = unsafe { manager.allocate(CapabilityFlags::empty(), rest) }.unwrap();
    assert_eq!(manager.get_free(), 0);

    // Shrinking by one granule hands it back, growing again takes it back.
    let granule = Layout::from_size_align(total / 2 - 2 * WORD, WORD).unwrap();
    let shrunk = unsafe { manager.reallocate(CapabilityFlags::empty(), a, half, granule) }.unwrap();
    assert_eq!(shrunk, a);
    assert_eq!(manager.get_free(), 2 * WORD);
    validate(&manager);
    assert_eq!(
        unsafe { manager.reallocate(CapabilityFlags::empty(), a, granule, half) },
        Some(a)
    );
    assert_eq!(manager.get_free(), 0);

    let smaller = Layout::from_size_align(total / 2 - WORD, WORD).unwrap();
    let shrunk = unsafe { manager.reallocate(CapabilityFlags::empty(), a, half, smaller) }.unwrap();
    assert_eq!(shrunk, a);
    unsafe {
        manager.deallocate(shrunk, smaller);
        manager.deallocate(b, rest);
    }
    validate(&manager);
    assert_eq!(
        manager.get_used(),
        0,
        "a word stays used after every block was freed"
    );
    assert_eq!(largest_allocation(&manager), total);
}

fn narrowed(pointer: NonNull<u8>, length: usize) -> NonNull<u8> {
    // What a caller that went through a reference (e.g. `Box::into_raw` of a `Box<[u8]>`) passes:
    // a pointer whose provenance only covers the block.
    let slice: &mut [u8] = unsafe { core::slice::from_raw_parts_mut(pointer.as_ptr(), length) };
    NonNull::from(slice).cast::<u8>()
}

/// The heap writes the moved hole header past the block: it must not do it through the caller's
/// pointer, which only covers the block. Only Miri (Stacked Borrows) can tell.
#[test]
fn provenance_in_place_grow_through_a_narrowed_pointer() {
    let manager = RegionManager::new([RegionDescriptor::new(CapabilityFlags::empty())], None);
    assert!(manager.init_region_from_slice(0, leak::<1024>()));
    let small = Layout::from_size_align(16, 8).unwrap();
    let large = Layout::from_size_align(64, 8).unwrap();
    let block = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
    let grown =
        unsafe { manager.reallocate(CapabilityFlags::empty(), narrowed(block, 16), small, large) }
            .unwrap();
    assert_eq!(grown, block);
    // The returned pointer must cover the grown block, not just the caller's 16 bytes.
    unsafe { grown.as_ptr().add(16).write_bytes(0xA5, 48) };
    unsafe { manager.deallocate(grown, large) };
}

/// The freed block's pointer becomes a hole node; a later allocation writes a hole header past
/// the old block through it.
#[test]
fn provenance_free_through_a_narrowed_pointer() {
    let manager = RegionManager::new([RegionDescriptor::new(CapabilityFlags::empty())], None);
    assert!(manager.init_region_from_slice(0, leak::<1024>()));
    let small = Layout::from_size_align(16, 8).unwrap();
    let a = unsafe { manager.allocate(CapabilityFlags::empty(), small) }.unwrap();
    unsafe { manager.deallocate(narrowed(a, 16), small) };
    let large = Layout::from_size_align(64, 8).unwrap();
    let b = unsafe { manager.allocate(CapabilityFlags::empty(), large) }.unwrap();
    assert_eq!(a, b);
    unsafe { manager.deallocate(b, large) };
}

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

struct Live {
    pointer: NonNull<u8>,
    layout: Layout,
    capabilities: CapabilityFlags,
    region: usize,
    seed: u8,
}

fn stamp(pointer: NonNull<u8>, length: usize, seed: u8) {
    for i in 0..length {
        unsafe { pointer.as_ptr().add(i).write(seed.wrapping_add(i as u8)) };
    }
}

fn stamped(pointer: NonNull<u8>, length: usize, seed: u8) -> bool {
    (0..length).all(|i| unsafe { pointer.as_ptr().add(i).read() } == seed.wrapping_add(i as u8))
}

#[derive(Default, Debug)]
struct Counts {
    allocations: usize,
    allocation_failures: usize,
    reallocation_failures: usize,
    grown_in_place: usize,
    moved: usize,
    left_region: usize,
    shrunk_in_place: usize,
}

const THRESHOLD: usize = 1024;

/// A random sequence of allocations, reallocations and frees over small regions, so that
/// exhaustion, failed reallocations and every placement path happen. Every invariant is checked
/// after every operation: exact `used` and `free` (per region and in total), the free lists,
/// overlap, and the data of every live block.
fn stress(seed: u64, operations: usize, counts: &mut Counts) {
    let manager = RegionManager::new(
        [
            RegionDescriptor::new(
                CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible,
            )
            .with_dma_alignment(4),
            RegionDescriptor::new(CapabilityFlags::DirectMemoryAccess).with_dma_alignment(64),
        ],
        Some(THRESHOLD),
    );
    assert!(manager.init_region_from_slice(0, leak::<6144>()));
    assert!(manager.init_region_from_slice(1, leak::<12288>()));

    let mut random = Rng(seed);
    let mut live: Vec<Live> = Vec::new();
    let mut expected_used = 0usize;
    let mut next_seed = 0u8;

    // DMA data keeps sizes and alignment valid for every region, plain data does not.
    let make_size = |random: &mut Rng, capabilities: CapabilityFlags| {
        if capabilities.contains(CapabilityFlags::DirectMemoryAccess) {
            (random.below(12) + 1) * 64
        } else if random.below(4) == 0 {
            random.below(3000) + 1
        } else {
            random.below(300) + 1
        }
    };

    for _ in 0..operations {
        let before_used = manager.get_used();
        match random.below(10) {
            0..=3 if live.len() < 80 => {
                let capabilities = [
                    CapabilityFlags::empty(),
                    CapabilityFlags::AlwaysAccessible,
                    CapabilityFlags::DirectMemoryAccess,
                ][random.below(3)];
                let size = make_size(&mut random, capabilities);
                let align = if capabilities.contains(CapabilityFlags::DirectMemoryAccess) {
                    64
                } else {
                    1 << random.below(7)
                };
                let layout = Layout::from_size_align(size, align).unwrap();
                let Some(pointer) = (unsafe { manager.allocate(capabilities, layout) }) else {
                    counts.allocation_failures += 1;
                    assert_eq!(manager.get_used(), before_used);
                    continue;
                };
                counts.allocations += 1;
                assert_eq!(pointer.as_ptr().addr() % align, 0);
                next_seed = next_seed.wrapping_add(37);
                stamp(pointer, size, next_seed);
                expected_used += footprint(size);
                let region = (0..2)
                    .find(|i| manager.region_contains(*i, pointer))
                    .unwrap();
                live.push(Live {
                    pointer,
                    layout,
                    capabilities,
                    region,
                    seed: next_seed,
                });
            }
            4..=5 if !live.is_empty() => {
                let block = live.swap_remove(random.below(live.len()));
                unsafe { manager.deallocate(block.pointer, block.layout) };
                expected_used -= footprint(block.layout.size());
            }
            _ if !live.is_empty() => {
                let index = random.below(live.len());
                let block = &mut live[index];
                let unit = if block
                    .capabilities
                    .contains(CapabilityFlags::DirectMemoryAccess)
                {
                    64
                } else {
                    WORD
                };
                let new_size = match random.below(4) {
                    0 => make_size(&mut random, block.capabilities),
                    1 => block.layout.size() + (random.below(8) + 1) * unit,
                    2 => block
                        .layout
                        .size()
                        .saturating_sub((random.below(8) + 1) * unit),
                    _ => block.layout.size().saturating_sub(unit),
                }
                .max(unit);
                let new_layout = Layout::from_size_align(new_size, block.layout.align()).unwrap();
                let old_pointer = block.pointer;
                let old_size = block.layout.size();

                let Some(pointer) = (unsafe {
                    manager.reallocate(block.capabilities, old_pointer, block.layout, new_layout)
                }) else {
                    counts.reallocation_failures += 1;
                    assert_eq!(manager.get_used(), before_used);
                    assert!(stamped(old_pointer, old_size, block.seed));
                    continue;
                };
                assert!(stamped(pointer, old_size.min(new_size), block.seed));
                assert_eq!(pointer.as_ptr().addr() % block.layout.align(), 0);
                let region = (0..2)
                    .find(|i| manager.region_contains(*i, pointer))
                    .unwrap();

                if block
                    .capabilities
                    .contains(CapabilityFlags::AlwaysAccessible)
                {
                    assert_eq!(region, 0);
                }
                if new_size <= old_size {
                    // A shrink never moves.
                    assert_eq!(pointer, old_pointer, "a shrink moved");
                    counts.shrunk_in_place += 1;
                } else if pointer == old_pointer {
                    counts.grown_in_place += 1;
                } else {
                    counts.moved += 1;
                }
                if region != block.region {
                    counts.left_region += 1;
                    // Only a growth can leave the region: a shrink never moves.
                    assert!(new_size > old_size);
                }

                expected_used = expected_used - footprint(old_size) + footprint(new_size);
                block.pointer = pointer;
                block.layout = new_layout;
                block.region = region;
                next_seed = next_seed.wrapping_add(37);
                block.seed = next_seed;
                stamp(pointer, new_size, next_seed);
            }
            _ => {}
        }

        // Every invariant, after every operation.
        assert_eq!(manager.get_used(), expected_used);
        validate(&manager);
        let mut sorted: Vec<(usize, usize)> = live
            .iter()
            .map(|block| {
                (
                    block.pointer.as_ptr().addr(),
                    footprint(block.layout.size()),
                )
            })
            .collect();
        sorted.sort();
        for pair in sorted.windows(2) {
            assert!(pair[0].0 + pair[0].1 <= pair[1].0, "blocks overlap");
        }
        for block in &live {
            assert!(stamped(block.pointer, block.layout.size(), block.seed));
        }
        let statistics = manager.get_statistics();
        let mut per_region_used = [0usize; 2];
        for block in &live {
            per_region_used[block.region] += footprint(block.layout.size());
        }
        for (region, stats) in statistics.region_stats.iter().enumerate() {
            let stats = stats.as_ref().unwrap();
            assert_eq!(stats.used, per_region_used[region], "region {region} used");
            assert_eq!(stats.used + stats.free, stats.size, "region {region} free");
        }
        assert_eq!(manager.get_free(), manager.get_total_size() - expected_used);
        #[cfg(feature = "Debug")]
        assert_eq!(
            statistics.total_allocated - statistics.total_freed,
            statistics.current_usage
        );
    }

    for block in live {
        unsafe { manager.deallocate(block.pointer, block.layout) };
    }
    validate(&manager);
    assert_eq!(manager.get_used(), 0);
    // Nothing stays stranded: the larger region serves a block of its own size again.
    assert_eq!(largest_allocation(&manager), 12288);
}

#[test]
fn random_reallocation_sequences_keep_every_invariant() {
    // About 6 s in release mode, 15 s in debug mode; Miri is far slower and a short run still
    // reaches every path.
    let seeds = if cfg!(miri) {
        1
    } else if cfg!(debug_assertions) {
        30
    } else {
        300
    };
    let operations = if cfg!(miri) { 300 } else { 4000 };
    let mut counts = Counts::default();
    for seed in 1..=seeds {
        let seed = 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(seed) | 1;
        stress(seed, operations, &mut counts);
    }
    // The sequences reached every path.
    assert!(
        counts.allocations > 0 && counts.allocation_failures > 0,
        "{counts:?}"
    );
    assert!(counts.reallocation_failures > 0, "{counts:?}");
    assert!(counts.grown_in_place > 0 && counts.moved > 0, "{counts:?}");
    assert!(
        counts.shrunk_in_place > 0 && counts.left_region > 0,
        "{counts:?}"
    );
}

/// Several threads resize their own blocks while others allocate: a block being copied must
/// never be handed out.
#[cfg(not(miri))]
#[test]
fn concurrent_reallocations_keep_data() {
    let manager: &'static RegionManager<2> =
        alloc::boxed::Box::leak(alloc::boxed::Box::new(RegionManager::new(
            [
                RegionDescriptor::new(CapabilityFlags::empty()),
                RegionDescriptor::new(CapabilityFlags::empty()),
            ],
            Some(2048),
        )));
    assert!(manager.init_region_from_slice(0, leak::<65536>()));
    assert!(manager.init_region_from_slice(1, leak::<65536>()));

    let threads: Vec<_> = (0..6u64)
        .map(|thread| {
            std::thread::spawn(move || {
                let mut random = Rng(0x1234_5678_9ABC_DEF1 ^ (thread + 1).wrapping_mul(0x9E37));
                let mut blocks: Vec<(NonNull<u8>, Layout, u8)> = Vec::new();
                for step in 0..60_000u32 {
                    let seed = (step as u8).wrapping_mul(7).wrapping_add(thread as u8);
                    match random.below(3) {
                        0 if blocks.len() < 20 => {
                            let layout = Layout::from_size_align(random.below(900) + 1, 8).unwrap();
                            if let Some(p) =
                                unsafe { manager.allocate(CapabilityFlags::empty(), layout) }
                            {
                                stamp(p, layout.size(), seed);
                                blocks.push((p, layout, seed));
                            }
                        }
                        1 if !blocks.is_empty() => {
                            let (p, layout, s) = blocks.swap_remove(random.below(blocks.len()));
                            assert!(stamped(p, layout.size(), s), "data corrupted");
                            unsafe { manager.deallocate(p, layout) };
                        }
                        _ if !blocks.is_empty() => {
                            let index = random.below(blocks.len());
                            let (p, layout, s) = blocks[index];
                            let new_layout =
                                Layout::from_size_align(random.below(2000) + 1, 8).unwrap();
                            if let Some(q) = unsafe {
                                manager.reallocate(CapabilityFlags::empty(), p, layout, new_layout)
                            } {
                                assert!(
                                    stamped(q, layout.size().min(new_layout.size()), s),
                                    "data corrupted by a concurrent operation"
                                );
                                stamp(q, new_layout.size(), seed);
                                blocks[index] = (q, new_layout, seed);
                            }
                        }
                        _ => {}
                    }
                }
                for (p, layout, s) in blocks {
                    assert!(stamped(p, layout.size(), s));
                    unsafe { manager.deallocate(p, layout) };
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    validate(manager);
    std::println!("used after all threads: {}", manager.get_used());
}

/// A one-word shrink of an `AlwaysAccessible` block in a full internal region used to move it to
/// PSRAM: a shrink that dropped the capability and copied the whole block. It now stays.
#[test]
fn a_one_word_shrink_keeps_an_always_accessible_block_in_internal_ram() {
    let manager = RegionManager::new(
        [
            RegionDescriptor::new(
                CapabilityFlags::DirectMemoryAccess | CapabilityFlags::AlwaysAccessible,
            ),
            RegionDescriptor::new(CapabilityFlags::DirectMemoryAccess).with_dma_alignment(64),
        ],
        Some(4096),
    );
    assert!(manager.init_region_from_slice(0, leak::<4096>()));
    assert!(manager.init_region_from_slice(1, leak::<16384>()));
    let internal = critical_section::with(|cs| manager.inner.borrow_ref(cs).regions[0].heap.size());

    let block_layout = Layout::from_size_align(1000, 8).unwrap();
    let block =
        unsafe { manager.allocate(CapabilityFlags::AlwaysAccessible, block_layout) }.unwrap();
    let rest = Layout::from_size_align(internal - footprint(1000), 8).unwrap();
    let neighbour = unsafe { manager.allocate(CapabilityFlags::AlwaysAccessible, rest) }.unwrap();
    assert!(manager.region_contains(0, block) && manager.region_contains(0, neighbour));

    let smaller = Layout::from_size_align(1000 - WORD, 8).unwrap();
    let shrunk = unsafe {
        manager.reallocate(
            CapabilityFlags::AlwaysAccessible,
            block,
            block_layout,
            smaller,
        )
    }
    .unwrap();
    assert_eq!(shrunk, block);
    let smaller_still = Layout::from_size_align(1000 - 4 * WORD, 8).unwrap();
    let shrunk = unsafe {
        manager.reallocate(
            CapabilityFlags::AlwaysAccessible,
            shrunk,
            smaller,
            smaller_still,
        )
    }
    .unwrap();
    assert_eq!(shrunk, block);
    assert!(manager.region_contains(0, shrunk));
    assert_eq!(
        manager.get_statistics().region_stats[1]
            .as_ref()
            .unwrap()
            .used,
        0
    );
}
