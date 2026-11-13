//! LVGL allocates from the allocator `include/lv_conf.h` selects: Xila's system
//! allocator on hosts, LVGL's fixed pool when built with
//! `LVGL_CFLAGS=XILA_LVGL_MEM_POOL=1` (run by `mise run test:graphics-pool`).
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use graphics::lvgl;

const BLOCK: usize = 64 * 1024;

fn system_used() -> usize {
    testing::memory_crate::global_manager().get_used()
}

fn monitor() -> lvgl::lv_mem_monitor_t {
    let mut monitor = lvgl::lv_mem_monitor_t::default();
    unsafe { lvgl::lv_mem_monitor(&mut monitor) };
    monitor
}

/// The pool size `lv_conf.h` was built with: bindgen cannot evaluate macros
/// passed on the command line, so read the same `LVGL_CFLAGS` the build used.
fn configured_pool_size() -> usize {
    std::env::var("LVGL_CFLAGS")
        .unwrap_or_default()
        .split(',')
        .find_map(|flag| flag.trim().strip_prefix("XILA_LVGL_MEM_SIZE="))
        .map_or(128 * 1024, |size| size.parse().unwrap())
}

#[test]
fn lvgl_allocations_come_from_the_selected_allocator() {
    unsafe { lvgl::lv_init() };

    let system_before = system_used();
    let before = monitor();
    let block = unsafe { lvgl::lv_malloc(BLOCK) };
    assert!(!block.is_null());
    let system_growth = system_used().saturating_sub(system_before);
    let after = monitor();

    assert!(
        before.free_size - after.free_size >= BLOCK,
        "lv_mem_monitor must account for the allocation"
    );
    if cfg!(lvgl_system_heap) {
        assert!(
            system_growth >= BLOCK,
            "LVGL must allocate from the system heap, which grew by {system_growth}"
        );
    } else {
        assert!(
            system_growth < BLOCK,
            "LVGL's pool must not touch the system heap, which grew by {system_growth}"
        );
        assert_eq!(after.used_cnt, before.used_cnt + 1);
        assert!(after.max_used >= BLOCK);
        // Configured by `XILA_LVGL_MEM_SIZE` (128 KiB by default), minus allocator overhead.
        let configured = configured_pool_size();
        assert!(
            (configured - 4096..=configured).contains(&before.total_size),
            "pool of {} bytes does not match LV_MEM_SIZE {configured}",
            before.total_size
        );
        // The pool is bounded: an allocation larger than the pool fails instead
        // of spilling into the system heap.
        assert!(unsafe { lvgl::lv_malloc(configured) }.is_null());
        assert_eq!(
            unsafe { lvgl::lv_mem_test() },
            lvgl::lv_result_t_LV_RESULT_OK
        );
    }

    // Reallocation and string duplication must stay in the selected heap.
    unsafe { core::ptr::write_bytes(block, 0x5a, BLOCK) };
    let resized = unsafe { lvgl::lv_realloc(block, BLOCK + 1024) };
    assert!(!resized.is_null());
    assert!(
        unsafe { core::slice::from_raw_parts(resized.cast::<u8>(), BLOCK) }
            .iter()
            .all(|byte| *byte == 0x5a)
    );
    unsafe { lvgl::lv_free(resized) };
    let before_strings = monitor();
    let duplicate = unsafe { lvgl::lv_strdup(c"graphics".as_ptr()) };
    let bounded = unsafe { lvgl::lv_strndup(c"graphics".as_ptr(), 5) };
    assert!(!duplicate.is_null() && !bounded.is_null());
    assert_eq!(unsafe { core::ffi::CStr::from_ptr(duplicate) }, c"graphics");
    assert_eq!(unsafe { core::ffi::CStr::from_ptr(bounded) }, c"graph");
    assert!(monitor().free_size < before_strings.free_size);
    unsafe {
        lvgl::lv_free(duplicate.cast());
        lvgl::lv_free(bounded.cast());
    }

    let freed = monitor();
    assert_eq!(freed.free_size, before.free_size);
}
