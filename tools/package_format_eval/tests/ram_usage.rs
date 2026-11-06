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

fn measure_peak_heap<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    LIVE.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
    TRACKING.store(true, Ordering::SeqCst);
    let result = operation();
    TRACKING.store(false, Ordering::SeqCst);
    (result, PEAK.load(Ordering::Relaxed))
}

#[test]
fn measure_peak_requested_heap_for_chunk_sizes_and_workloads() {
    let corpus = package_format_eval::fixtures(402).unwrap();
    let candidates = package_format_eval::build_candidate_archives(&corpus).unwrap();
    let raw_candidates =
        package_format_eval::build_candidate_archives_uncompressed(&corpus).unwrap();

    for chunk_bytes in [512, 1024, 4096] {
        for (candidate, raw_candidate) in candidates.iter().zip(&raw_candidates) {
            let (_, peak_heap) = measure_peak_heap(|| {
                drop(package_format_eval::probe_candidate(candidate, chunk_bytes).unwrap());
            });
            println!(
                "format={} chunk_bytes={chunk_bytes} measured_peak_parser_heap_bytes={peak_heap} source_archive_bytes={} scenario=sequential_scan_all_entries",
                candidate.format.name(),
                candidate.bytes.len()
            );
            assert!(peak_heap > 0, "parser allocations must be observed");

            let (_, selected_peak_heap) = measure_peak_heap(|| {
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

            let temp_root = std::env::temp_dir().join(format!(
                "xila-measured-extraction-{}-{}",
                std::process::id(),
                candidate.format.name()
            ));
            let _ = std::fs::remove_dir_all(&temp_root);
            std::fs::create_dir_all(&temp_root).unwrap();
            let source_path = temp_root.join("source.pkg");
            std::fs::write(&source_path, &raw_candidate.bytes).unwrap();

            let full_output = temp_root.join("full");
            std::fs::create_dir_all(&full_output).unwrap();
            let (full_report, full_peak_heap) = measure_peak_heap(|| {
                package_format_eval::extract_candidate_file_backed(
                    raw_candidate.format,
                    &source_path,
                    &full_output,
                    chunk_bytes,
                    None,
                )
                .unwrap()
            });
            println!(
                "format={} chunk_bytes={chunk_bytes} measured_peak_extraction_heap_bytes={full_peak_heap} files_written={} bytes_written={} scenario=file_backed_extract_all",
                candidate.format.name(),
                full_report.files_written,
                full_report.bytes_written
            );
            assert_eq!(full_report.files_written, corpus.len());
            for fixture in &corpus {
                assert_eq!(
                    std::fs::read(full_output.join(&fixture.path)).unwrap(),
                    fixture.data
                );
            }

            let selected_output = temp_root.join("selected");
            std::fs::create_dir_all(&selected_output).unwrap();
            let (selected_report, selected_file_peak_heap) = measure_peak_heap(|| {
                package_format_eval::extract_candidate_file_backed(
                    raw_candidate.format,
                    &source_path,
                    &selected_output,
                    chunk_bytes,
                    Some("bin/application.wasm"),
                )
                .unwrap()
            });
            println!(
                "format={} chunk_bytes={chunk_bytes} measured_peak_extraction_heap_bytes={selected_file_peak_heap} files_written={} bytes_written={} scenario=file_backed_extract_selected:bin/application.wasm",
                candidate.format.name(),
                selected_report.files_written,
                selected_report.bytes_written
            );
            assert_eq!(selected_report.files_written, 1);
            assert_eq!(
                std::fs::read(selected_output.join("bin/application.wasm")).unwrap(),
                corpus[0].data
            );
            std::fs::remove_dir_all(temp_root).unwrap();
        }
    }
}
