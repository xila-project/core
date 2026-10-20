use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct PeakAllocator;

static TRACKING: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: PeakAllocator = PeakAllocator;

unsafe impl GlobalAlloc for PeakAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && TRACKING.load(Ordering::Relaxed) {
            record_delta(layout.size(), 0);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if TRACKING.load(Ordering::Relaxed) {
            record_delta(0, layout.size());
        }
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let new_pointer = unsafe { System.realloc(pointer, old, new_size) };
        if !new_pointer.is_null() && TRACKING.load(Ordering::Relaxed) {
            record_delta(new_size, old.size());
        }
        new_pointer
    }
}

fn record_delta(added: usize, removed: usize) {
    LIVE.fetch_add(added, Ordering::Relaxed);
    let live = LIVE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_sub(removed))
        })
        .unwrap_or(0);
    let live = live.saturating_sub(removed);
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn measure_peak_heap(operation: impl FnOnce()) -> usize {
    LIVE.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
    TRACKING.store(true, Ordering::SeqCst);
    operation();
    TRACKING.store(false, Ordering::SeqCst);
    PEAK.load(Ordering::Relaxed)
}

#[test]
fn measure_peak_requested_heap_for_chunk_sizes_and_workloads() {
    let candidates =
        package_format_eval::build_candidate_archives(&package_format_eval::fixtures(402).unwrap())
            .unwrap();

    for chunk_bytes in [512, 1024, 4096] {
        for candidate in &candidates {
            let peak_heap = measure_peak_heap(|| {
                drop(package_format_eval::probe_candidate(candidate, chunk_bytes).unwrap());
            });
            println!(
                "format={} chunk_bytes={chunk_bytes} measured_peak_parser_heap_bytes={peak_heap} source_archive_bytes={} scenario=sequential_scan_all_entries",
                candidate.format.name(),
                candidate.bytes.len()
            );
            assert!(peak_heap > 0, "parser allocations must be observed");

            let selected_peak_heap = measure_peak_heap(|| {
                drop(
                    package_format_eval::probe_candidate_for_path(
                        candidate,
                        chunk_bytes,
                        Some("bin/application.wasm"),
                    )
                    .unwrap(),
                );
            });
            println!(
                "format={} chunk_bytes={chunk_bytes} measured_peak_parser_heap_bytes={selected_peak_heap} source_archive_bytes={} scenario=consume_selected_file_payload:bin/application.wasm",
                candidate.format.name(),
                candidate.bytes.len()
            );
            assert!(
                selected_peak_heap > 0,
                "selected-file parser allocations must be observed"
            );
        }
    }
}
