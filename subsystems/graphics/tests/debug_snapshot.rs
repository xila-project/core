#![cfg(target_os = "linux")]

extern crate abi_definitions;

drivers_std::instantiate_global_allocator!();

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
    let time = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));
    let manager = task::block_on(graphics::initialize_with_input_devices(
        time,
        &Screen,
        &[],
        1024,
        false,
    ));
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
        let checkbox = lvgl::lv_checkbox_create(screen);
        lvgl::lv_checkbox_set_text(checkbox, c"Remember".as_ptr());
        lvgl::lv_obj_add_state(checkbox, lvgl::lv_state_t_LV_STATE_CHECKED);
    }
    let snapshot = task::block_on(manager.inspect_ui_tree(64, 16)).unwrap();
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
    // The graphics manager and its LVGL display live for the runtime lifetime.
    core::mem::forget(manager);
}
