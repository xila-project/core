use memory::{Layout, RegionHeap};

fn with_heap(test: impl FnOnce(&mut RegionHeap)) {
    let mut backing = vec![0u128; 8192];
    let mut heap = RegionHeap::empty();
    unsafe { heap.init(backing.as_mut_ptr().cast(), backing.len() * 16) };
    test(&mut heap);
}

#[test]
fn alignment_exhaustion_and_statistics() {
    with_heap(|heap| {
        let initial = heap.statistics();
        assert_eq!(initial.used, 0);
        for align in [1, 2, 4, 8, 16, 32, 64, 128] {
            let layout = Layout::from_size_align(137, align).unwrap();
            let p = heap.allocate(layout).unwrap();
            assert_eq!(p.as_ptr() as usize % align, 0);
            assert!(heap.used() >= 137);
            assert_eq!(heap.used() + heap.free(), initial.size);
            unsafe { heap.deallocate(p, layout) };
            assert_eq!(heap.free(), initial.free);
        }
        assert!(
            heap.allocate(Layout::from_size_align(1 << 20, 8).unwrap())
                .is_none()
        );
        let layout = Layout::from_size_align(256, 8).unwrap();
        let mut pointers = Vec::new();
        while let Some(p) = heap.allocate(layout) {
            pointers.push(p);
        }
        assert!(heap.allocate(layout).is_none());
        for p in pointers {
            unsafe { heap.deallocate(p, layout) };
        }
        assert_eq!(heap.statistics().free, initial.free);
    });
}

#[test]
fn reallocation_in_place_moving_alignment_and_failure() {
    with_heap(|heap| unsafe {
        let small = Layout::from_size_align(128, 8).unwrap();
        let large = Layout::from_size_align(1024, 8).unwrap();
        let p = heap.allocate(small).unwrap();
        p.as_ptr().write_bytes(0x5a, 128);
        let grown = heap.reallocate(p, small, large).unwrap();
        assert_eq!(p, grown);
        let shrunk = heap.reallocate(grown, large, small).unwrap();
        assert_eq!(p, shrunk);
        let barrier = heap.allocate(small).unwrap();
        let moved = heap.reallocate(shrunk, small, large).unwrap();
        assert_ne!(moved, shrunk);
        assert_eq!(
            core::slice::from_raw_parts(moved.as_ptr(), 128),
            &[0x5a; 128]
        );
        assert!(
            heap.reallocate(moved, large, Layout::from_size_align(1 << 20, 8).unwrap())
                .is_none()
        );
        assert_eq!(*moved.as_ptr(), 0x5a);
        let aligned = Layout::from_size_align(2048, 64).unwrap();
        let moved = heap.reallocate(moved, large, aligned).unwrap();
        assert_eq!(moved.as_ptr() as usize % 64, 0);
        assert_eq!(*moved.as_ptr(), 0x5a);
        heap.deallocate(moved, aligned);
        heap.deallocate(barrier, small);
        assert_eq!(heap.used(), 0);
    });
}

#[test]
fn unaligned_and_incremental_extension() {
    let mut backing = vec![0u128; 4096];
    let mut heap = RegionHeap::<{ usize::BITS as usize }>::empty();
    unsafe { heap.init(backing.as_mut_ptr().cast::<u8>().add(1), 4095) };
    let layout = Layout::from_size_align(2048, 64).unwrap();
    let p = heap.allocate(layout).unwrap();
    let before = heap.free();
    for _ in 0..4096 {
        unsafe { heap.extend(1) };
    }
    assert!(heap.free() > before);
    unsafe { heap.deallocate(p, layout) };
    assert_eq!(heap.used(), 0);
}

#[test]
fn growth_budget_covers_size_class_rounding() {
    for size in [1, 16, 255, 256, 4096, 8192, 32768, 65536] {
        for align in [1, 8, 64, 256] {
            let layout = Layout::from_size_align(size, align).unwrap();
            let budget = RegionHeap::<16>::growth_size(layout).unwrap();
            let mut backing = vec![0u128; budget.div_ceil(16) + 1];
            let mut heap = RegionHeap::<16>::empty();
            unsafe { heap.init(backing.as_mut_ptr().cast::<u8>().add(1), budget) };
            let p = heap
                .allocate(layout)
                .expect("growth budget must cover TLSF bin rounding");
            unsafe { heap.deallocate(p, layout) };
        }
    }
}
