//! Attribution of invalidated areas, for rendering telemetry.

use core::ffi::CStr;

use crate::lvgl;

/// Class name of the object that most likely invalidated `area` on `display`.
///
/// LVGL reports an invalidated area (`LV_EVENT_INVALIDATE_AREA`) without the
/// object it belongs to. An object invalidates at most its own area, so this is
/// the deepest visible object that contains the area, searched from the topmost
/// layer down: a child that covers its parent exactly is named in its place.
/// It is `None` when no object of the display contains the area.
///
/// The area may be up to one pixel larger than the object on each side, as
/// displays that round invalidated areas make it.
///
/// # Safety
/// `display` must be live and the caller must hold the LVGL lock, as a display
/// event callback does.
pub unsafe fn get_invalidated_class_name(
    display: *mut lvgl::lv_display_t,
    area: &lvgl::lv_area_t,
) -> Option<&'static CStr> {
    unsafe {
        let screen = lvgl::lv_display_get_screen_active(display);
        let object = [
            lvgl::lv_display_get_layer_sys(display),
            lvgl::lv_display_get_layer_top(display),
            screen,
            lvgl::lv_display_get_layer_bottom(display),
        ]
        .into_iter()
        .filter(|root| !root.is_null())
        .find_map(|root| {
            let deepest = find_deepest(root, area);
            // A layer covers the display and draws nothing itself: only what it
            // holds counts. The screen draws the background.
            (deepest != root || (root == screen && contains(root, area))).then_some(deepest)
        })?;
        let name = (*lvgl::lv_obj_get_class(object)).name;
        (!name.is_null()).then(|| CStr::from_ptr(name))
    }
}

/// Follows, from `root`, the topmost visible child that contains `area`.
unsafe fn find_deepest(root: *mut lvgl::lv_obj_t, area: &lvgl::lv_area_t) -> *mut lvgl::lv_obj_t {
    let mut object = root;
    loop {
        let child = unsafe {
            (0..lvgl::lv_obj_get_child_count(object) as i32)
                .rev()
                .map(|index| lvgl::lv_obj_get_child(object, index))
                .find(|child| contains(*child, area))
        };
        match child {
            Some(child) => object = child,
            None => return object,
        }
    }
}

unsafe fn contains(object: *mut lvgl::lv_obj_t, area: &lvgl::lv_area_t) -> bool {
    unsafe {
        if lvgl::lv_obj_has_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) {
            return false;
        }
        let mut coordinates = lvgl::lv_area_t::default();
        lvgl::lv_obj_get_coords(object, &mut coordinates);
        // What the object draws outside its coordinates, such as an outline.
        let extra = lvgl::lv_obj_get_ext_draw_size(object) + 1;
        coordinates.x1 - extra <= area.x1
            && coordinates.y1 - extra <= area.y1
            && coordinates.x2 + extra >= area.x2
            && coordinates.y2 + extra >= area.y2
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn invalidated_areas_are_attributed_to_the_deepest_object_that_holds_them() {
        unsafe {
            lvgl::lv_init();
            let display = lvgl::lv_display_create(320, 240);
            let screen = lvgl::lv_display_get_screen_active(display);
            let card = lvgl::lv_obj_create(screen);
            lvgl::lv_obj_set_pos(card, 10, 10);
            lvgl::lv_obj_set_size(card, 200, 100);
            let button = lvgl::lv_button_create(card);
            lvgl::lv_obj_set_size(button, 80, 30);
            let label = lvgl::lv_label_create(button);
            lvgl::lv_label_set_text(label, c"Open".as_ptr());
            let overlay = lvgl::lv_switch_create(lvgl::lv_display_get_layer_top(display));
            lvgl::lv_obj_set_pos(overlay, 120, 60);
            lvgl::lv_obj_add_flag(overlay, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            lvgl::lv_obj_update_layout(screen);

            let area_of = |object| {
                let mut area = lvgl::lv_area_t::default();
                lvgl::lv_obj_get_coords(object, &mut area);
                area
            };
            let name = |area: lvgl::lv_area_t| {
                get_invalidated_class_name(display, &area).map(|name| name.to_str().unwrap())
            };
            assert_eq!(name(area_of(label)), Some("lv_label"));
            assert_eq!(name(area_of(button)), Some("lv_button"));
            // Rounded to an even width, as some displays do.
            let mut rounded = area_of(button);
            rounded.x1 -= 1;
            rounded.x2 += 1;
            assert_eq!(name(rounded), Some("lv_button"));
            assert_eq!(name(area_of(card)), Some("lv_obj"));
            assert_eq!(name(area_of(screen)), Some("lv_obj"));
            // An object of the top layer wins over the card under it, once it
            // shows.
            assert_eq!(name(area_of(overlay)), Some("lv_obj"));
            lvgl::lv_obj_remove_flag(overlay, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            assert_eq!(name(area_of(overlay)), Some("lv_switch"));

            lvgl::lv_display_delete(display);
            lvgl::lv_deinit();
        }
    }
}
