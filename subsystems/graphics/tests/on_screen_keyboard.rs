#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

// LVGL owns process-global state. Keep this test in its own test binary.
#[path = "../src/on_screen_keyboard.rs"]
mod on_screen_keyboard;
use graphics::lvgl;

#[test]
fn keyboard_opens_on_click_and_detaches_before_deletion() {
    use lvgl::*;
    use on_screen_keyboard::OnScreenKeyboard;
    use std::ptr;

    unsafe {
        lv_init();
        let display = lv_display_create(480, 320);
        let keyboard_state = OnScreenKeyboard::new();
        let screen = lv_display_get_screen_active(display);
        assert!(!on_screen_keyboard::enabled_for(screen));
        keyboard_state.observe(display);
        assert!(on_screen_keyboard::enabled_for(screen));
        let top = lv_display_get_layer_top(display);
        let child_count = lv_obj_get_child_count(top);

        let group = lv_group_create();
        lv_group_set_default(group);
        let window = lv_obj_create(screen);
        lv_obj_set_size(window, lv_pct(100), lv_pct(100));
        let first = lv_textarea_create(window);
        lv_obj_align(first, lv_align_t_LV_ALIGN_BOTTOM_MID, 0, 0);
        assert_eq!(lv_group_get_focused(group), first);
        let second = lv_textarea_create(window);
        keyboard_state.update(display);
        assert_eq!(lv_obj_get_child_count(top), child_count);

        lv_obj_send_event(first, lv_event_code_t_LV_EVENT_CLICKED, ptr::null_mut());
        keyboard_state.update(display);
        let keyboard = lv_obj_get_child(top, -1);
        assert_eq!(lv_obj_get_child_count(top), child_count + 1);
        assert_eq!(lv_keyboard_get_textarea(keyboard), first);
        assert_eq!(lv_obj_get_height(keyboard), 160);
        let mut field_bounds = lv_area_t::default();
        let mut keyboard_bounds = lv_area_t::default();
        lv_obj_get_coords(first, &mut field_bounds);
        lv_obj_get_coords(keyboard, &mut keyboard_bounds);
        assert!(field_bounds.y2 < keyboard_bounds.y1);

        lv_obj_send_event(first, lv_event_code_t_LV_EVENT_READY, ptr::null_mut());
        keyboard_state.update(display);
        assert!(lv_keyboard_get_textarea(keyboard).is_null());
        assert!(lv_obj_has_flag(keyboard, lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN));

        lv_obj_send_event(first, lv_event_code_t_LV_EVENT_CLICKED, ptr::null_mut());
        keyboard_state.update(display);
        assert_eq!(lv_keyboard_get_textarea(keyboard), first);
        lv_obj_send_event(first, lv_event_code_t_LV_EVENT_DEFOCUSED, ptr::null_mut());
        lv_obj_send_event(second, lv_event_code_t_LV_EVENT_CLICKED, ptr::null_mut());
        keyboard_state.update(display);
        assert_eq!(lv_keyboard_get_textarea(keyboard), second);

        lv_obj_delete(window);
        assert!(lv_keyboard_get_textarea(keyboard).is_null());
        assert!(lv_obj_has_flag(keyboard, lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN));
        keyboard_state.update(display);
        let replacement = lv_obj_create(ptr::null_mut());
        lv_screen_load(replacement);
        let field = lv_textarea_create(replacement);
        lv_obj_send_event(field, lv_event_code_t_LV_EVENT_CLICKED, ptr::null_mut());
        keyboard_state.update(display);
        assert_eq!(lv_keyboard_get_textarea(keyboard), field);
        lv_screen_load(screen);
        lv_obj_delete(replacement);
        assert!(lv_keyboard_get_textarea(keyboard).is_null());
        assert!(lv_obj_has_flag(keyboard, lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN));
        keyboard_state.update(display);

        lv_deinit();
    }
}
