#![cfg(all(
    feature = "host",
    feature = "graphics",
    feature = "memory_32",
    not(feature = "memory_64")
))]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
#[path = "support/localization.rs"]
mod localization;
#[path = "support/graphics.rs"]
mod support;

use core::{
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};
use xila::{
    graphics::{Manager, lvgl},
    virtual_file_system::File,
};

async fn status(manager: &Manager, expected: &str) {
    let expected = localization::text("pixel_painter", expected);
    let mut observed = String::new();
    for _ in 0..400 {
        if manager.get_window_count().await.unwrap() > 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let _lock = manager.synchronous_lock();
            unsafe {
                let label = lvgl::lv_obj_get_child(root, 4);
                if !label.is_null() {
                    observed = core::ffi::CStr::from_ptr(lvgl::lv_label_get_text(label))
                        .to_string_lossy()
                        .into_owned();
                    if observed == expected {
                        return;
                    }
                }
            }
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Pixel Painter did not show {expected:?}; observed {observed:?}");
}

async fn action(manager: &Manager, button: u32) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let matrix = lvgl::lv_obj_get_child(root, 1);
        lvgl::lv_buttonmatrix_set_selected_button(matrix, button);
        lvgl::lv_obj_send_event(
            matrix,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    }
}

#[xila::task::test(task_path=xila::task)]
async fn real_painter_guest_draws_saves_opens_and_preserves_work_on_errors() {
    let mut standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let context = support::context(&standard, Some(manager));
    standard.context = context;
    let task = context.task_manager.get_current_task_identifier().await;
    let vfs = context.virtual_file_system;
    vfs.ensure_user_application_directories(
        task,
        xila::users::UserIdentifier::ROOT,
        "pixel_painter",
    )
    .await
    .unwrap();
    let binary = xila::executable::build_crate("pixel_painter").unwrap();
    drivers_std::loader::load_to_virtual_file_system(
        context.task_manager,
        vfs,
        binary,
        "/applications/executables/pixel_painter",
    )
    .await
    .unwrap();
    xila::executable::mount_executables!(
        context,
        vfs,
        task,
        &[("/applications/executables/wasm", wasm::WasmExecutable)]
    )
    .await
    .unwrap();
    let handle = xila::executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/applications/executables/pixel_painter".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    status(manager, "status.ready_white_erases").await;
    // Queue two distinct pointer positions before the guest drains either one.
    // Re-reading the current input position at dequeue time would lose the start.
    {
        let _lock = manager.synchronous_lock();
        let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
        unsafe {
            lvgl::lv_obj_update_layout(root);
            let canvas = lvgl::lv_obj_get_child(root, 3);
            let mut area = lvgl::lv_area_t::default();
            lvgl::lv_obj_get_content_coords(canvas, &mut area);
            assert!(area.y2 > area.y1, "canvas needs usable height");
            let input = lvgl::lv_indev_create();
            lvgl::lv_indev_set_type(input, lvgl::lv_indev_type_t_LV_INDEV_TYPE_POINTER);
            let x = (area.x1 + area.x2) / 2;
            let y = (area.y1 + area.y2) / 2;
            (*input).pointer.act_point = lvgl::lv_point_t { x: x - 5, y };
            lvgl::lv_obj_send_event(canvas, lvgl::lv_event_code_t_LV_EVENT_PRESSED, input.cast());
            (*input).pointer.act_point.x = x + 5;
            lvgl::lv_obj_send_event(
                canvas,
                lvgl::lv_event_code_t_LV_EVENT_PRESSING,
                input.cast(),
            );
            lvgl::lv_obj_send_event(
                canvas,
                lvgl::lv_event_code_t_LV_EVENT_RELEASED,
                input.cast(),
            );
            lvgl::lv_indev_delete(input);
        }
    }
    xila::task::sleep(Duration::from_millis(50)).await;
    action(manager, 2).await;
    status(manager, "status.saved").await;
    let path = "/users/0/applications/pixel_painter/data/drawing.xpnt";
    let mut saved = Vec::new();
    File::read_from_path(vfs, task, path, &mut saved)
        .await
        .unwrap();
    assert_eq!(saved.len(), 16_008);
    assert!(
        saved[8..].iter().filter(|&&pixel| pixel == 1).count() > 1,
        "pointer positions must produce a continuous stroke"
    );
    action(manager, 1).await; // Clear.
    xila::task::sleep(Duration::from_millis(30)).await;
    action(manager, 0).await; // Undo clear.
    xila::task::sleep(Duration::from_millis(30)).await;
    action(manager, 2).await;
    status(manager, "status.saved").await;
    let mut after_undo = Vec::new();
    File::read_from_path(vfs, task, path, &mut after_undo)
        .await
        .unwrap();
    assert_eq!(after_undo, saved);
    action(manager, 3).await;
    status(manager, "status.opened").await;
    File::write_to_path(vfs, task, path, b"broken")
        .await
        .unwrap();
    action(manager, 3).await;
    status(manager, "errors.invalid_file_drawing_preserved").await;
    action(manager, 2).await;
    status(manager, "status.saved").await;
    File::read_from_path(vfs, task, path, &mut after_undo)
        .await
        .unwrap();
    assert_eq!(after_undo, saved);
    action(manager, 1).await;
    xila::task::sleep(Duration::from_millis(30)).await;
    action(manager, 3).await;
    status(manager, "warnings.unsaved_open_again").await;
    action(manager, 3).await;
    status(manager, "status.opened").await;
    let root = manager.get_window_identifier(0).await.unwrap();
    {
        let _lock = manager.synchronous_lock();
        let long_path = std::ffi::CString::new(format!("/{}", "é".repeat(130))).unwrap();
        unsafe {
            let input = lvgl::lv_obj_get_child(root as *mut lvgl::lv_obj_t, 2);
            lvgl::lv_textarea_set_text(input, long_path.as_ptr());
        }
    }
    action(manager, 2).await;
    status(manager, "errors.save_failed_drawing_preserved").await;
    {
        let _lock = manager.synchronous_lock();
        let path = std::ffi::CString::new(path).unwrap();
        unsafe {
            let input = lvgl::lv_obj_get_child(root as *mut lvgl::lv_obj_t, 2);
            lvgl::lv_textarea_set_text(input, path.as_ptr());
        }
    }
    action(manager, 2).await;
    status(manager, "status.saved").await;
    File::read_from_path(vfs, task, path, &mut after_undo)
        .await
        .unwrap();
    assert_eq!(after_undo, saved);
    action(manager, 1).await;
    xila::task::sleep(Duration::from_millis(30)).await;
    manager.send_window_close_request(root).await.unwrap();
    status(manager, "warnings.unsaved_close_again").await;
    manager.send_window_close_request(root).await.unwrap();
    let mut join = core::pin::pin!(handle.join());
    for _ in 0..400 {
        if let Poll::Ready(exit) = poll_fn(|cx| Poll::Ready(join.as_mut().poll(cx))).await {
            assert_eq!(exit, 0);
            assert_eq!(manager.get_window_count().await.unwrap(), 0);
            return;
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Pixel Painter did not close");
}
