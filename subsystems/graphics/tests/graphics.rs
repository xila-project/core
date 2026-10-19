extern crate alloc;

// Import abi_definitions to ensure ABI function definitions are linked
extern crate abi_definitions;

#[cfg(target_os = "linux")]
use task::test;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
drivers_std::memory::instantiate_global_allocator!();

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[test]
#[ignore]
async fn main() {
    use std::{process::exit, ptr::null_mut};

    use drivers_native::window_screen;
    use graphics::{InputKind, Point, get_recommended_buffer_size, lvgl};

    let task_instance = Box::leak(Box::new(task::Manager::new()));
    let _users = users::Manager::new();
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));

    const RESOLUTION: Point = Point::new(800, 480);

    const BUFFER_SIZE: usize = get_recommended_buffer_size(&RESOLUTION);

    let (screen_device, pointer_device, keyboard_device, mut runner) =
        window_screen::new(RESOLUTION).await.unwrap();

    let _task = task_instance.get_current_task_identifier().await;

    let graphics = graphics::initialize(
        time_manager,
        Box::leak(Box::new(screen_device)),
        Box::leak(Box::new(pointer_device)),
        InputKind::Pointer,
        BUFFER_SIZE,
        true,
    )
    .await;

    graphics
        .add_input_device(Box::leak(Box::new(keyboard_device)), InputKind::Keypad)
        .await
        .unwrap();

    task_instance
        .spawn(_task, "Window screen runner", None, async move |_| {
            runner.run().await;
        })
        .await
        .unwrap();

    let mut window = graphics.create_window().await.unwrap();

    let window = window.as_object_mutable();

    let _calendar = unsafe { lvgl::lv_calendar_create(window) };

    let button = unsafe { lvgl::lv_button_create(window) };

    let label = unsafe { lvgl::lv_label_create(button) };

    let slider = unsafe { lvgl::lv_slider_create(window) };

    unsafe extern "C" fn quit(_event: *mut lvgl::lv_event_t) {
        exit(0);
    }

    unsafe {
        lvgl::lv_obj_set_align(slider, lvgl::lv_align_t_LV_ALIGN_LEFT_MID);
        lvgl::lv_obj_set_align(button, lvgl::lv_align_t_LV_ALIGN_CENTER);
        lvgl::lv_label_set_text(label, c"Quit".as_ptr());
        lvgl::lv_obj_add_event_cb(
            button,
            Some(quit),
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            null_mut(),
        );
    }

    graphics.r#loop(task::Manager::sleep).await.unwrap();
}
