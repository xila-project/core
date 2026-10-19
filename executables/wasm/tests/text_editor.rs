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

use core::time::Duration;
use xila::{
    file_system::{AccessFlags, CreateFlags, Flags},
    graphics::{Manager, lvgl},
    virtual_file_system::{File, SynchronousDirectory},
};

async fn wait_label(manager: &Manager, child: i32, expected: &str) {
    let mut observed = String::new();
    for _ in 0..400 {
        if manager.get_window_count().await.unwrap() > 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let _lock = manager.synchronous_lock();
            unsafe {
                let label = lvgl::lv_obj_get_child(root, child);
                if !label.is_null() {
                    observed = core::ffi::CStr::from_ptr(lvgl::lv_label_get_text(label))
                        .to_string_lossy()
                        .into_owned();
                    if observed.contains(expected) {
                        return;
                    }
                }
            }
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Text Editor did not show {expected:?}; observed {observed:?}");
}

async fn action(manager: &Manager, button: u32) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let matrix = lvgl::lv_obj_get_child(root, 0);
        lvgl::lv_buttonmatrix_set_selected_button(matrix, button);
        lvgl::lv_obj_send_event(
            matrix,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    }
}

#[xila::task::test(task_path = xila::task)]
async fn guest_opens_pages_edits_and_saves_large_file() {
    let mut standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let context = support::context(&standard, Some(manager));
    standard.context = context;
    let task = context.task_manager.get_current_task_identifier().await;
    let vfs = context.virtual_file_system;
    vfs.create_directory(task, &"/configuration/shared/text_editor")
        .await
        .unwrap();
    let path = "/configuration/shared/text_editor/document.txt";
    let original = "a".repeat(9000);
    let mut source = File::open(
        vfs,
        task,
        path,
        Flags::new(AccessFlags::Write, Some(CreateFlags::CreateTruncate), None),
    )
    .await
    .unwrap();
    let mut remaining = original.as_bytes();
    while !remaining.is_empty() {
        let written = source.write(remaining).await.unwrap();
        assert_ne!(written, 0);
        remaining = &remaining[written..];
    }
    source.close(vfs).await.unwrap();
    let binary = xila::executable::build_crate("text_editor").unwrap();
    drivers_std::loader::load_to_virtual_file_system(
        context.task_manager,
        vfs,
        binary,
        "/binaries/text_editor",
    )
    .await
    .unwrap();
    xila::executable::mount_executables!(
        context,
        vfs,
        task,
        &[("/binaries/wasm", wasm::WasmExecutable)]
    )
    .await
    .unwrap();
    let handle = xila::executable::execute(
        context,
        "/binaries/wasm",
        vec!["/binaries/text_editor".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    wait_label(manager, 4, "Choose a path").await;
    let path_text = std::ffi::CString::new(path).unwrap();
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let path_input = lvgl::lv_obj_get_child(root, 1);
            lvgl::lv_textarea_set_text(path_input, path_text.as_ptr());
        }
    }
    action(manager, 3).await;
    wait_label(manager, 4, "Opened").await;
    xila::task::sleep(Duration::from_millis(30)).await;
    action(manager, 1).await;
    wait_label(manager, 2, "4096 | 4096 bytes").await;
    xila::task::sleep(Duration::from_millis(30)).await;
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let text = lvgl::lv_obj_get_child(root, 3);
            lvgl::lv_textarea_set_text(text, c"edited".as_ptr());
        }
    }
    action(manager, 4).await;
    wait_label(manager, 4, "Saved").await;
    xila::task::sleep(Duration::from_millis(30)).await;
    let mut saved = Vec::new();
    File::read_from_path(vfs, task, path, &mut saved)
        .await
        .unwrap();
    assert_eq!(
        saved,
        format!("{}edited{}", "a".repeat(4096), "a".repeat(9000 - 8192)).as_bytes()
    );
    let window = manager.get_window_identifier(0).await.unwrap();
    manager.send_window_close_request(window).await.unwrap();
    let exit = handle.join().await;
    assert_eq!(exit, 0);
    let directory =
        SynchronousDirectory::open(vfs, task, "/configuration/shared/text_editor").unwrap();
    let names: Vec<_> = directory.map(|entry| entry.name).collect();
    assert!(names.iter().any(|name| name == "document.txt"));
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with(".xila-text-editor-"))
    );
}
