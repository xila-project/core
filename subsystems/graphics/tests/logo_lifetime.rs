//! A self-drawing logo must be freed with its LVGL window.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing_support::instantiate_global_allocator!();

use graphics::{Color, Logo, LogoArmPose, LogoPose, Window, lvgl};
use testing_support::MemoryManager;

#[test]
fn deleting_a_window_frees_its_logo() {
    unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(320, 240);
        let screen = lvgl::lv_display_get_screen_active(display);
        let cycle = |release: fn(Logo)| {
            let window = Window::new(screen).unwrap().as_ptr().cast();
            let logo = Logo::new(window, 3, Color::WHITE).unwrap();
            assert_eq!(lvgl::lv_obj_get_child_count(logo.get_inner_object()), 0);
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

        animated_logo_draws_beyond_its_resting_bounds(screen);
        lvgl::lv_display_delete(display);
        lvgl::lv_deinit();
    }
}

unsafe fn animated_logo_draws_beyond_its_resting_bounds(screen: *mut lvgl::lv_obj_t) {
    unsafe {
        lvgl::lv_obj_set_style_bg_color(screen, Color::BLACK.into_lvgl_color(), 0);
        lvgl::lv_obj_set_style_bg_opa(screen, 255, 0);
        let logo = Logo::new(screen, 1, Color::WHITE).unwrap();
        let object = logo.get_inner_object();
        lvgl::lv_obj_set_pos(object, 64, 64);
        lvgl::lv_obj_update_layout(screen);
        let format = lvgl::lv_color_format_t_LV_COLOR_FORMAT_XRGB8888;
        let buffer = lvgl::lv_snapshot_create_draw_buf(screen, format);
        assert!(!buffer.is_null());
        let sample = || {
            assert_eq!(
                lvgl::lv_snapshot_take_to_draw_buf(screen, format, buffer),
                lvgl::lv_result_t_LV_RESULT_OK,
            );
            let offset = 60 * (*buffer).header.stride() as usize + 57 * 4;
            *(*buffer).data.add(offset)
        };

        assert_eq!(sample(), 0);
        let mut pose = LogoPose::REST;
        pose.arms[0] = LogoArmPose {
            offset: 8,
            opacity: 255,
        };
        Logo::set_pose(object, pose);
        assert_eq!(lvgl::lv_obj_get_ext_draw_size(object), 8);
        assert_eq!(sample(), 255);

        Logo::set_pose(object, LogoPose::REST);
        assert_eq!(lvgl::lv_obj_get_ext_draw_size(object), 0);
        assert_eq!(sample(), 0);
        lvgl::lv_draw_buf_destroy(buffer);
    }
}
