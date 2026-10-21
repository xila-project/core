#![cfg(all(
    feature = "host",
    feature = "graphics",
    feature = "memory_32",
    not(feature = "memory_64")
))]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
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

unsafe fn find_class(
    root: *mut lvgl::lv_obj_t,
    class: *const lvgl::lv_obj_class_t,
    ordinal: usize,
    found: &mut usize,
) -> *mut lvgl::lv_obj_t {
    unsafe {
        if lvgl::lv_obj_has_class(root, class) {
            if *found == ordinal {
                return root;
            }
            *found += 1;
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let object = find_class(
                lvgl::lv_obj_get_child(root, index as i32),
                class,
                ordinal,
                found,
            );
            if !object.is_null() {
                return object;
            }
        }
        core::ptr::null_mut()
    }
}

unsafe fn find_label(root: *mut lvgl::lv_obj_t, expected: &str) -> *mut lvgl::lv_obj_t {
    unsafe {
        if lvgl::lv_obj_has_class(root, &lvgl::lv_label_class) {
            let text = core::ffi::CStr::from_ptr(lvgl::lv_label_get_text(root)).to_bytes();
            if text == expected.as_bytes() {
                return root;
            }
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let label = find_label(lvgl::lv_obj_get_child(root, index as i32), expected);
            if !label.is_null() {
                return label;
            }
        }
        core::ptr::null_mut()
    }
}

async fn status(manager: &Manager, expected: &str) {
    let mut observed = String::new();
    for _ in 0..400 {
        if manager.get_window_count().await.unwrap() > 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let _lock = manager.synchronous_lock();
            unsafe {
                let label = find_label(root, expected);
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
        let mut found = 0;
        let matrix = find_class(root, &lvgl::lv_buttonmatrix_class, 0, &mut found);
        assert!(
            !matrix.is_null(),
            "Pixel Painter should contain its action matrix"
        );
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
    status(manager, "Ready - white erases").await;
    // Queue two distinct pointer positions before the guest drains either one.
    // Re-reading the current input position at dequeue time would lose the start.
    {
        let _lock = manager.synchronous_lock();
        let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
        unsafe {
            lvgl::lv_obj_update_layout(root);
            // Pixel Painter creates its direct canvas after the path field.
            let canvas = lvgl::lv_obj_get_child(root, 4);
            assert!(
                !canvas.is_null(),
                "Pixel Painter should contain its drawing canvas"
            );
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
    status(manager, "Saved").await;
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
    status(manager, "Saved").await;
    let mut after_undo = Vec::new();
    File::read_from_path(vfs, task, path, &mut after_undo)
        .await
        .unwrap();
    assert_eq!(after_undo, saved);
    action(manager, 3).await;
    status(manager, "Opened").await;
    File::write_to_path(vfs, task, path, b"broken")
        .await
        .unwrap();
    action(manager, 3).await;
    status(manager, "Invalid Pixel Painter file - drawing kept").await;
    action(manager, 2).await;
    status(manager, "Saved").await;
    File::read_from_path(vfs, task, path, &mut after_undo)
        .await
        .unwrap();
    assert_eq!(after_undo, saved);
    action(manager, 1).await;
    xila::task::sleep(Duration::from_millis(30)).await;
    action(manager, 3).await;
    status(manager, "Unsaved drawing - Open again to discard").await;
    action(manager, 3).await;
    status(manager, "Opened").await;
    let root = manager.get_window_identifier(0).await.unwrap();
    {
        let _lock = manager.synchronous_lock();
        let long_path = std::ffi::CString::new(format!("/{}", "é".repeat(130))).unwrap();
        unsafe {
            let mut found = 0;
            let input = find_class(
                root as *mut lvgl::lv_obj_t,
                &lvgl::lv_textarea_class,
                0,
                &mut found,
            );
            assert!(
                !input.is_null(),
                "Pixel Painter should contain a path field"
            );
            lvgl::lv_textarea_set_text(input, long_path.as_ptr());
        }
    }
    action(manager, 2).await;
    status(
        manager,
        "Save failed - drawing kept; check path and storage",
    )
    .await;
    {
        let _lock = manager.synchronous_lock();
        let path = std::ffi::CString::new(path).unwrap();
        unsafe {
            let mut found = 0;
            let input = find_class(
                root as *mut lvgl::lv_obj_t,
                &lvgl::lv_textarea_class,
                0,
                &mut found,
            );
            assert!(
                !input.is_null(),
                "Pixel Painter should contain a path field"
            );
            lvgl::lv_textarea_set_text(input, path.as_ptr());
        }
    }
    action(manager, 2).await;
    status(manager, "Saved").await;
    File::read_from_path(vfs, task, path, &mut after_undo)
        .await
        .unwrap();
    assert_eq!(after_undo, saved);
    action(manager, 1).await;
    xila::task::sleep(Duration::from_millis(30)).await;
    manager.send_window_close_request(root).await.unwrap();
    status(manager, "Unsaved drawing - close again to discard").await;
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
