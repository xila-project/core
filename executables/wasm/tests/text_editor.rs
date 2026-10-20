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
    graphics::{EventKind, Manager, Window, lvgl},
    virtual_file_system::{File, SynchronousDirectory},
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
            if text
                .windows(expected.len())
                .any(|window| window == expected.as_bytes())
            {
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

async fn wait_label(manager: &Manager, expected: &str) {
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
        let mut window = Window::from_raw(root).unwrap();
        while window.as_mut().pop_event().is_some() {}
        let mut found = 0;
        let matrix = find_class(root, &lvgl::lv_buttonmatrix_class, 0, &mut found);
        assert!(
            !matrix.is_null(),
            "Text Editor should contain its action matrix"
        );
        lvgl::lv_buttonmatrix_set_selected_button(matrix, button);
        assert_eq!(lvgl::lv_buttonmatrix_get_selected_button(matrix), button);
        assert_eq!(
            lvgl::lv_obj_send_event(
                matrix,
                lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
                core::ptr::null_mut(),
            ),
            lvgl::lv_result_t_LV_RESULT_OK
        );
        let event = window.as_ref().peek_event().unwrap();
        assert_eq!(event.code, EventKind::ValueChanged);
        assert_eq!(event.target, matrix);
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
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
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
    let mut source_contents = Vec::new();
    File::read_from_path(vfs, task, path, &mut source_contents)
        .await
        .unwrap();
    assert_eq!(source_contents, original.as_bytes());
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
    wait_label(manager, "Choose a path").await;
    let path_text = std::ffi::CString::new(path).unwrap();
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let mut found = 0;
            let path_input = find_class(root, &lvgl::lv_textarea_class, 0, &mut found);
            assert!(
                !path_input.is_null(),
                "Text Editor should contain a path field"
            );
            lvgl::lv_textarea_set_text(path_input, path_text.as_ptr());
        }
    }
    action(manager, 3).await;
    wait_label(manager, "Opened").await;
    wait_label(manager, "0 | 4096 of 9000 bytes").await;
    xila::task::sleep(Duration::from_millis(30)).await;
    action(manager, 1).await;
    wait_label(manager, "4096 | 4096 of 9000 bytes").await;
    xila::task::sleep(Duration::from_millis(30)).await;
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let mut found = 0;
            let text = find_class(root, &lvgl::lv_textarea_class, 1, &mut found);
            assert!(
                !text.is_null(),
                "Text Editor should contain its document field"
            );
            lvgl::lv_textarea_set_text(text, c"edited".as_ptr());
        }
    }
    action(manager, 4).await;
    wait_label(manager, "Saved").await;
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
