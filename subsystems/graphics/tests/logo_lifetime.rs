//! A logo's draw buffers must be freed with its LVGL window.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use graphics::{Color, Logo, Window, lvgl};
use testing::MemoryManager;

#[test]
fn deleting_a_window_frees_its_logo() {
    unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(320, 240);
        let screen = lvgl::lv_display_get_screen_active(display);
        let cycle = |release: fn(Logo)| {
            let window = Window::new(screen).unwrap().as_ptr().cast();
            let logo = Logo::new(window, 3, Color::WHITE).unwrap();
            release(logo);
            lvgl::lv_obj_delete(window);
        };
        let pool_free_bytes = || {
            let mut monitor = lvgl::lv_mem_monitor_t::default();
            lvgl::lv_mem_monitor(&mut monitor);
            monitor.free_size
        };

        cycle(drop); // Warm up LVGL before measuring.
        for release in [drop as fn(Logo), core::mem::forget, |logo| {
            let parent = lvgl::lv_obj_get_parent(logo.get_inner_object());
            let object = logo.into_inner_object();
            assert_eq!(lvgl::lv_obj_get_child(parent, -1), object);
        }] {
            let free_before = pool_free_bytes();
            let before = MemoryManager::thread_allocations();
            cycle(release);
            assert_eq!(
                MemoryManager::thread_allocations()
                    .since(before)
                    .net_bytes(),
                0
            );
            if !cfg!(lvgl_system_heap) {
                assert_eq!(pool_free_bytes(), free_before);
            }
        }

        lvgl::lv_display_delete(display);
        lvgl::lv_deinit();
    }
}
