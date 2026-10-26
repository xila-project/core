// Explicit drop boundaries release borrowed NOR state to model a reboot.
#![allow(clippy::drop_non_drop)]
#![cfg(feature = "simulator")]
mod support;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountingAllocator;

fn count() {
    ALLOCATIONS.with(|counter| {
        if let Some(value) = counter.get() {
            counter.set(Some(value + 1));
        }
    });
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measured(f: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|counter| counter.set(Some(0)));
    f();
    ALLOCATIONS.with(|counter| counter.replace(None).unwrap())
}

#[test]
fn update_hash_signature_readback_and_boot_paths_do_not_allocate() {
    use firmware_update::nor::MemoryNor;
    use firmware_update::simulator::{FLASH_SIZE, Simulator};
    use firmware_update::{FirmwareBackend, Update};
    use support::*;
    assert_eq!(
        measured(|| {
            std::hint::black_box(Box::new(17u64));
        }),
        1
    );
    let key = key().verifying_key().to_bytes();
    let old = manifest(b"old", 1);
    let new = manifest(&[0x5a; 137], 2);
    let mut bytes = [255; FLASH_SIZE];
    let count = measured(|| {
        Simulator::provision(
            MemoryNor::new(&mut bytes).unwrap(),
            key,
            TARGET,
            &old,
            b"old",
        )
        .unwrap();
        let mut u =
            Update::new(Simulator::new(MemoryNor::new(&mut bytes).unwrap(), key, TARGET).unwrap());
        u.begin(137, &new).unwrap();
        let mut off = 0;
        while off < 137 {
            off += u.write(&[0x5a; 137][off..]).unwrap();
        }
        u.finish().unwrap();
        u.activate().unwrap();
        drop(u);
        let mut sim = Simulator::new(MemoryNor::new(&mut bytes).unwrap(), key, TARGET).unwrap();
        sim.boot().unwrap();
        sim.confirm_running().unwrap();
    });
    assert_eq!(count, 0);
    println!(
        "native bytes: simulator={} update={} flash={} caller_buffer=64; zero heap requests in measured core/backend path (not peak stack)",
        std::mem::size_of::<Simulator<MemoryNor<'_>>>(),
        std::mem::size_of::<Update<Simulator<MemoryNor<'_>>>>(),
        FLASH_SIZE
    );
}
