#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use core::task::{Context, Poll};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::{GET_RESOLUTION, Point, SET_DRAWING_AREA, WAS_RESIZED, lvgl};
use shared::AnyByLayout;

struct Screen;
impl MountOperations for Screen {}
impl DirectCharacterDevice for Screen {}
impl DirectBaseOperations for Screen {
    fn read(&self, _: &mut Context<'_>, _: &mut [u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::Error::UnsupportedOperation))
    }
    fn write(
        &self,
        _: &mut Context<'_>,
        bytes: &[u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Ok(bytes.len()))
    }
    fn control(
        &self,
        _: &mut Context<'_>,
        command: ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<file_system::Result<()>> {
        Poll::Ready(match command {
            GET_RESOLUTION::IDENTIFIER => GET_RESOLUTION::cast_output(output)
                .map(|resolution| *resolution = Point::new(320, 240)),
            WAS_RESIZED::IDENTIFIER => {
                WAS_RESIZED::cast_output(output).map(|resized| *resized = false)
            }
            SET_DRAWING_AREA::IDENTIFIER => Ok(()),
            _ => Err(file_system::Error::UnsupportedOperation),
        })
    }
}

#[test]
fn snapshot_describes_widget_roles_labels_and_explicit_state() {
    let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let manager = task::block_on(graphics::initialize_with_input_devices(
        time,
        &Screen,
        &[],
        1024,
        false,
    ));
    // The safe session and raw compatibility API share one LVGL and lock.
    use graphics::lvgl_rust::{Obj, Owner, WidgetExt};
    let owner = Owner::new(1).unwrap();
    let handle = manager
        .with_lvgl_sync(|lv| {
            let mut screen = lv.screen_active_mut().unwrap();
            Ok(Obj::create(&mut screen)?.handle_for(owner)?)
        })
        .unwrap();
    manager
        .with_lvgl_sync(|lv| {
            assert!(lv.resolve_for::<Obj>(handle.into_raw(), owner).is_ok());
            assert_eq!(lv.delete_owner(owner), 1);
            assert!(lv.resolve_for::<Obj>(handle.into_raw(), owner).is_err());
            Ok(())
        })
        .unwrap();
    let screen = manager.get_current_screen().unwrap();
    unsafe {
        let button = lvgl::lv_button_create(screen);
        let label = lvgl::lv_label_create(button);
        lvgl::lv_label_set_text(label, c"Open".as_ptr());
        lvgl::lv_obj_add_flag(button, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
        lvgl::lv_obj_add_state(button, lvgl::lv_state_t_LV_STATE_DISABLED);
        let textbox = lvgl::lv_textarea_create(screen);
        lvgl::lv_textarea_set_placeholder_text(textbox, c"Username".as_ptr());
        lvgl::lv_textarea_set_text(textbox, c"Ada".as_ptr());
        // LVGL expects each key as a native-endian packed UTF-8 sequence.
        let mut accented = u32::from_ne_bytes([0xc3, 0xa9, 0, 0]);
        let mut cjk = u32::from_ne_bytes([0xe4, 0xb8, 0xad, 0]);
        lvgl::lv_obj_send_event(
            textbox,
            lvgl::lv_event_code_t_LV_EVENT_KEY,
            (&mut accented as *mut u32).cast(),
        );
        lvgl::lv_obj_send_event(
            textbox,
            lvgl::lv_event_code_t_LV_EVENT_KEY,
            (&mut cjk as *mut u32).cast(),
        );
        assert_eq!(
            std::ffi::CStr::from_ptr(lvgl::lv_textarea_get_text(textbox))
                .to_str()
                .unwrap(),
            "Adaé中"
        );
        let capped = lvgl::lv_textarea_create(screen);
        let long_placeholder = std::ffi::CString::new("n".repeat(300)).unwrap();
        lvgl::lv_textarea_set_placeholder_text(capped, long_placeholder.as_ptr());
        lvgl::lv_textarea_set_text(capped, c"bounded".as_ptr());
        let unicode = lvgl::lv_label_create(screen);
        let utf8 = std::ffi::CString::new(format!("{}中", "a".repeat(511))).unwrap();
        lvgl::lv_label_set_text(unicode, utf8.as_ptr());
        let checkbox = lvgl::lv_checkbox_create(screen);
        lvgl::lv_checkbox_set_text(checkbox, c"Remember".as_ptr());
        lvgl::lv_obj_add_state(checkbox, lvgl::lv_state_t_LV_STATE_CHECKED);
    }
    unsafe {
        let password = lvgl::lv_textarea_create(screen);
        lvgl::lv_textarea_set_placeholder_text(password, c"Password".as_ptr());
        lvgl::lv_textarea_set_password_mode(password, true);
        lvgl::lv_textarea_set_text(password, c"do-not-export".as_ptr());
        // Include a revealed child, not just the default masked label.
        let reveal = lvgl::lv_label_create(password);
        lvgl::lv_label_set_text(reveal, c"do-not-export".as_ptr());
        let nested_button = lvgl::lv_button_create(password);
        let nested_label = lvgl::lv_label_create(nested_button);
        lvgl::lv_label_set_text(nested_label, c"do-not-export".as_ptr());
    }
    unsafe {
        let top = lvgl::lv_layer_top();
        let status = lvgl::lv_label_create(top);
        lvgl::lv_label_set_text(status, c"Top overlay".as_ptr());
        let slider = lvgl::lv_slider_create(screen);
        lvgl::lv_slider_set_range(slider, -10, 80);
        lvgl::lv_slider_set_value(slider, 42, false);
        lvgl::lv_obj_set_style_pad_top(slider, 7, 0);
        let matrix = lvgl::lv_buttonmatrix_create(screen);
        static MAP: [&core::ffi::CStr; 3] = [c"One", c"Two", c""];
        // LVGL retains the map pointers. Leak only in this process-lifetime fixture.
        let long_button = Box::leak(
            std::ffi::CString::new(format!("{}中", "b".repeat(127)))
                .unwrap()
                .into_boxed_c_str(),
        );
        let map = Box::leak(Box::new([
            MAP[0].as_ptr(),
            MAP[1].as_ptr(),
            long_button.as_ptr(),
            MAP[2].as_ptr(),
        ]));
        lvgl::lv_buttonmatrix_set_map(matrix, map.as_ptr());
        let dropdown = lvgl::lv_dropdown_create(screen);
        let long_option = std::ffi::CString::new(format!("{}中", "d".repeat(511))).unwrap();
        lvgl::lv_dropdown_set_options(dropdown, long_option.as_ptr());
        let offscreen = lvgl::lv_label_create(screen);
        lvgl::lv_label_set_text(offscreen, c"Offscreen".as_ptr());
        lvgl::lv_obj_set_pos(offscreen, 1000, 1000);
    }
    // Shell mounting must not hide screen siblings or overlay roots from Debug.
    let body = unsafe { lvgl::lv_obj_create(screen) };
    task::block_on(manager.set_window_parent(body)).unwrap();
    let snapshot = task::block_on(manager.inspect_ui_tree(64, 16)).unwrap();
    let overlay = unsafe {
        let overlay = lvgl::lv_obj_create(lvgl::lv_layer_top());
        lvgl::lv_obj_set_pos(overlay, 0, 0);
        lvgl::lv_obj_set_size(overlay, 20, 20);
        lvgl::lv_obj_set_style_bg_color(overlay, lvgl::lv_color_hex(0xff0000), 0);
        lvgl::lv_obj_set_style_bg_opa(overlay, 255, 0);
        lvgl::lv_obj_set_style_border_width(overlay, 0, 0);
        lvgl::lv_obj_set_style_radius(overlay, 0, 0);
        overlay
    };
    let capture = task::block_on(manager.capture_screen()).unwrap();
    unsafe {
        lvgl::lv_obj_set_style_bg_opa(overlay, 128, 0);
    }
    let translucent = task::block_on(manager.capture_screen()).unwrap();
    let create_target = || unsafe {
        let button = lvgl::lv_button_create(screen);
        lvgl::lv_obj_set_pos(button, 100, 100);
        lvgl::lv_obj_set_size(button, 100, 40);
        let label = lvgl::lv_label_create(button);
        lvgl::lv_label_set_text(label, c"Activate".as_ptr());
    };
    create_target();
    assert!(task::block_on(manager.click_ui(Some("button"), Some("Activate"), None, None)).is_ok());
    let cover = unsafe {
        let cover = lvgl::lv_obj_create(lvgl::lv_layer_top());
        lvgl::lv_obj_set_pos(cover, 100, 100);
        lvgl::lv_obj_set_size(cover, 100, 40);
        cover
    };
    assert_eq!(
        task::block_on(manager.click_ui(Some("button"), Some("Activate"), None, None)),
        Err("target_obscured")
    );
    unsafe {
        lvgl::lv_obj_delete(cover);
    }
    create_target();
    assert_eq!(
        task::block_on(manager.click_ui(Some("button"), Some("Activate"), None, None)),
        Err("ambiguous_target")
    );
    assert_eq!(
        task::block_on(manager.click_ui(None, None, None, None)),
        Err("invalid_selector")
    );
    core::mem::forget(manager);
    let offset = (10 * usize::from(capture.width) + 10) * 4;
    assert_eq!(
        &capture.pixels_rgba[offset..offset + 4],
        &[255, 0, 0, 255],
        "capture must composite the top layer"
    );
    assert_eq!(
        &translucent.pixels_rgba[offset..offset + 4],
        &[140, 12, 13, 255],
        "straight alpha overlays must blend without dark edges"
    );
    let button = snapshot
        .elements
        .iter()
        .find(|element| element.semantic_role == Some("button"))
        .unwrap();
    assert_eq!(button.name.as_deref(), Some("Open"));
    assert!(button.hidden && button.disabled);
    let label = snapshot
        .elements
        .iter()
        .find(|element| element.text.as_deref() == Some("Open"))
        .unwrap();
    assert!(label.hidden, "hidden state inherits from ancestors");
    let textbox = snapshot
        .elements
        .iter()
        .find(|element| element.semantic_role == Some("textbox"))
        .unwrap();
    assert_eq!(textbox.name.as_deref(), Some("Username"));
    assert_eq!(textbox.text.as_deref(), Some("Adaé中"));
    let capped = snapshot
        .elements
        .iter()
        .find(|element| element.text.as_deref() == Some("bounded"))
        .unwrap();
    assert_eq!(capped.name.as_ref().unwrap().len(), 128);
    let checkbox = snapshot
        .elements
        .iter()
        .find(|element| element.semantic_role == Some("checkbox"))
        .unwrap();
    assert_eq!(checkbox.name.as_deref(), Some("Remember"));
    assert_eq!(checkbox.checked, Some(true));
    assert!(
        snapshot
            .elements
            .iter()
            .any(|e| e.layer == "top" && e.text.as_deref() == Some("Top overlay"))
    );
    let slider = snapshot
        .elements
        .iter()
        .find(|e| e.semantic_role == Some("slider"))
        .unwrap();
    assert_eq!(slider.value, Some(42));
    assert_eq!(slider.range, Some((-10, 80)));
    assert_eq!(slider.style.padding.0, 7);
    let matrix = snapshot
        .elements
        .iter()
        .find(|e| e.role == "lv_buttonmatrix")
        .unwrap();
    assert_eq!(matrix.buttons.len(), 3);
    assert_eq!(matrix.buttons[2].text.as_ref().unwrap().len(), 127);
    assert!(matrix.buttons[2].text_truncated);
    assert_eq!(matrix.selected, None);
    let dropdown = snapshot
        .elements
        .iter()
        .find(|e| e.role == "lv_dropdown")
        .unwrap();
    assert_eq!(dropdown.selected_text.as_ref().unwrap().len(), 511);
    assert!(dropdown.selected_text_truncated);
    assert_eq!(matrix.buttons[0].text.as_deref(), Some("One"));
    assert!(matrix.buttons[0].bounds.0 <= matrix.buttons[0].bounds.2);
    let offscreen = snapshot
        .elements
        .iter()
        .find(|e| e.text.as_deref() == Some("Offscreen"))
        .unwrap();
    assert!(!offscreen.visible);
    assert!(offscreen.clipped_bounds.is_none());
    let unicode = snapshot
        .elements
        .iter()
        .find(|e| {
            e.text
                .as_ref()
                .is_some_and(|t| t.starts_with(&"a".repeat(100)))
        })
        .unwrap();
    assert_eq!(unicode.text.as_ref().unwrap().len(), 511);
    assert!(
        unicode.text_truncated,
        "a UTF-8 boundary trim must still report truncated input"
    );
    assert!(capped.name_truncated);
    assert!(
        snapshot
            .elements
            .iter()
            .filter(|e| e.redacted && e.name.is_none())
            .all(|e| !e.name_truncated)
    );
    assert!(!snapshot.elements.iter().any(|e| {
        e.name
            .as_deref()
            .is_some_and(|t| t.contains("do-not-export"))
    }));
    assert!(
        !snapshot.elements.iter().any(|e| e
            .text
            .as_deref()
            .is_some_and(|t| t.contains("do-not-export"))),
        "password descendants must not leak into snapshots"
    );
}
