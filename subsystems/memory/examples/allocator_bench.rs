//! Deterministic synthetic UI churn; run with `cargo run -p xila-memory --release --example allocator_bench`.
use memory::{Layout, RegionHeap};
use std::{ptr::NonNull, time::Instant};

trait Allocator {
    fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>>;
    unsafe fn free(&mut self, p: NonNull<u8>, layout: Layout);
    fn available(&self) -> usize;
}
impl Allocator for RegionHeap {
    fn alloc(&mut self, l: Layout) -> Option<NonNull<u8>> {
        self.allocate(l)
    }
    unsafe fn free(&mut self, p: NonNull<u8>, l: Layout) {
        unsafe { self.deallocate(p, l) };
    }
    fn available(&self) -> usize {
        self.free()
    }
}
impl Allocator for linked_list_allocator::Heap {
    fn alloc(&mut self, l: Layout) -> Option<NonNull<u8>> {
        self.allocate_first_fit(l).ok()
    }
    unsafe fn free(&mut self, p: NonNull<u8>, l: Layout) {
        unsafe { self.deallocate(p, l) };
    }
    fn available(&self) -> usize {
        self.free()
    }
}
fn random(seed: &mut u64) -> usize {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed as usize
}
fn run(name: &str, size: usize, mut heap: impl Allocator) {
    let mut seed = 0x123456789abcdefu64;
    let mut slots = vec![None; 512];
    let mut alloc_times = Vec::new();
    let mut free_times = Vec::new();
    let mut failures = 0;
    for step in 0..100_000 {
        let slot = random(&mut seed) % slots.len();
        if let Some((p, l)) = slots[slot].take() {
            let start = Instant::now();
            unsafe { heap.free(p, l) };
            free_times.push(start.elapsed().as_nanos());
        }
        let r = random(&mut seed);
        let bytes = if r.is_multiple_of(32) {
            1024 + (r / 32 % 8) * 1024
        } else {
            16 + (r / 32 % 16) * 16
        };
        let layout = Layout::from_size_align(bytes, 8).unwrap();
        let start = Instant::now();
        let p = heap.alloc(layout);
        alloc_times.push(start.elapsed().as_nanos());
        if let Some(p) = p {
            unsafe { p.as_ptr().write_bytes((step % 256) as u8, bytes) };
            slots[slot] = Some((p, layout));
        } else {
            failures += 1;
        }
    }
    // Snapshot with mixed-lifetime objects still live, before draining.
    let free = heap.available();
    // Exhaustive descending probe avoids assuming TLSF's class rounding is monotonic.
    let largest = (1..=free)
        .rev()
        .find(|&n| {
            let l = Layout::from_size_align(n, 8).unwrap();
            if let Some(p) = heap.alloc(l) {
                unsafe { heap.free(p, l) };
                true
            } else {
                false
            }
        })
        .unwrap_or(0);
    let distribution = |mut times: Vec<u128>| {
        times.sort_unstable();
        (
            times.iter().sum::<u128>() / times.len() as u128,
            times[times.len() * 99 / 100],
            *times.last().unwrap(),
        )
    };
    let a = distribution(alloc_times);
    let d = distribution(free_times);
    println!(
        "{name},{size},{failures},{free},{largest},{},{},{},{},{},{}",
        a.0, a.1, a.2, d.0, d.1, d.2
    );
    for (p, l) in slots.into_iter().flatten() {
        unsafe { heap.free(p, l) };
    }
}
fn main() {
    println!(
        "allocator,heap,failures,free,largest,alloc_avg_ns,alloc_p99_ns,alloc_max_ns,free_avg_ns,free_p99_ns,free_max_ns"
    );
    for size in [65536, 262144] {
        let mut backing = vec![0u128; size / 16];
        let mut old = linked_list_allocator::Heap::empty();
        unsafe { old.init(backing.as_mut_ptr().cast(), size) };
        run("first-fit", size, old);
        let mut new = RegionHeap::empty();
        unsafe { new.init(backing.as_mut_ptr().cast(), size) };
        run("tlsf", size, new);
    }
    eprintln!(
        "control bytes: first-fit={}, TLSF={}",
        std::mem::size_of::<linked_list_allocator::Heap>(),
        std::mem::size_of::<RegionHeap>()
    );
}
