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
fn verification_paths_do_not_allocate() {
    // Positive control: this instrumentation must detect a real allocation.
    assert_eq!(
        measured(|| {
            std::hint::black_box(Box::new(17u64));
        }),
        1
    );
    let count = measured(|| {
        #[cfg(feature = "sha256")]
        {
            use verification::{ContentVerifier, Sha256Verifier};
            let mut hash = Sha256Verifier::new();
            for _ in 0..8192 {
                hash.update(&[0x5a; 512]).unwrap();
            }
            let digest = hash.finalize().unwrap();
            let mut content = ContentVerifier::new(4 * 1024 * 1024, digest).unwrap();
            for _ in 0..8192 {
                content.update(&[0x5a; 512]).unwrap();
            }
            content.verify().unwrap();
            assert!(ContentVerifier::new(0, [0; 32]).unwrap().verify().is_err());
            let mut failed = ContentVerifier::new(0, digest).unwrap();
            assert!(failed.update(b"x").is_err());
            assert!(failed.verify().is_err());
            assert!(ContentVerifier::new(1, digest).unwrap().verify().is_err());
        }
        #[cfg(feature = "ed25519")]
        {
            use verification::PublicKey;
            // RFC 8032 section 7.1 TEST 1.
            let key = PublicKey::from_bytes(&[
                0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
                0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
                0xf7, 0x07, 0x51, 0x1a,
            ])
            .unwrap();
            let signature = [
                0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72, 0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e,
                0x82, 0x8a, 0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74, 0xd8, 0x73, 0xe0, 0x65,
                0x22, 0x49, 0x01, 0x55, 0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac, 0xc6, 0x1e,
                0x39, 0x70, 0x1c, 0xf9, 0xb4, 0x6b, 0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24,
                0x65, 0x51, 0x41, 0x43, 0x8e, 0x7a, 0x10, 0x0b,
            ];
            key.verify(b"", &signature).unwrap();
            assert!(key.verify(b"modified", &signature).is_err());
            assert!(key.verify_slice(b"", &[0; 63]).is_err());
            assert!(PublicKey::from_bytes(&[0; 32]).is_err());
            assert!(PublicKey::from_slice(&[0; 31]).is_err());
        }
    });
    assert_eq!(count, 0);
}
