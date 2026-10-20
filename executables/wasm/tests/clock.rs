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

use core::{ffi::CStr, future::poll_fn, task::Poll, time::Duration};
use xila::graphics::{Manager, lvgl};

unsafe fn find_label(root: *mut lvgl::lv_obj_t, text: &str, contains: bool) -> *mut lvgl::lv_obj_t {
    unsafe {
        if lvgl::lv_obj_has_class(root, &lvgl::lv_label_class) {
            let label = CStr::from_ptr(lvgl::lv_label_get_text(root)).to_bytes();
            if if contains {
                label
                    .windows(text.len())
                    .any(|window| window == text.as_bytes())
            } else {
                label == text.as_bytes()
            } {
                return root;
            }
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let found = find_label(lvgl::lv_obj_get_child(root, index as i32), text, contains);
            if !found.is_null() {
                return found;
            }
        }
        core::ptr::null_mut()
    }
}

async fn wait_label(manager: &Manager, text: &str, contains: bool) {
    for _ in 0..1000 {
        if manager.get_window_count().await.unwrap() != 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let found = {
                let _lock = manager.synchronous_lock();
                unsafe { !find_label(root, text, contains).is_null() }
            };
            if found {
                return;
            }
        }
        xila::task::sleep(Duration::from_millis(10)).await;
    }
    panic!("Clock did not display {text:?}");
}

async fn click(manager: &Manager, tab: Option<i32>, text: &str) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let parent = if let Some(index) = tab {
            let tabs = lvgl::lv_obj_get_child(root, 2);
            assert_eq!(lvgl::lv_tabview_get_tab_count(tabs), 4);
            lvgl::lv_tabview_set_active(tabs, index as u32, false);
            lvgl::lv_obj_get_child(lvgl::lv_tabview_get_content(tabs), index)
        } else {
            root
        };
        let label = find_label(parent, text, false);
        assert!(!label.is_null(), "missing Clock control {text:?}");
        let button = lvgl::lv_obj_get_parent(label);
        lvgl::lv_obj_send_event(
            button,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    }
}

#[xila::task::test(task_path=xila::task)]
async fn real_clock_guest_runs_stopwatch_timer_saves_alarm_and_closes() {
    let mut standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let context = support::context(&standard, Some(manager));
    standard.context = context;
    let task = context.task_manager.get_current_task_identifier().await;
    let binary = xila::executable::build_crate("clock").unwrap();
    drivers_std::loader::load_to_virtual_file_system(
        context.task_manager,
        context.virtual_file_system,
        binary,
        "/applications/executables/clock",
    )
    .await
    .unwrap();
    xila::executable::mount_executables!(
        context,
        context.virtual_file_system,
        task,
        &[("/applications/executables/wasm", wasm::WasmExecutable)]
    )
    .await
    .unwrap();
    // Launch as a login user; the launcher provisions their private app settings.
    let administrator = context
        .users_manager
        .get_user_identifier("administrator")
        .await
        .unwrap();
    context
        .task_manager
        .set_user(task, administrator)
        .await
        .unwrap();
    let handle = xila::executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/applications/executables/clock".into()],
        standard.duplicate().await.unwrap(),
        None,
    )
    .await
    .unwrap();
    wait_label(manager, "Cancel", true).await;
    click(manager, Some(2), "Start / pause").await;
    xila::task::sleep(Duration::from_millis(300)).await;
    click(manager, Some(2), "Lap").await;
    wait_label(manager, "1: ", true).await;
    click(manager, Some(2), "Start / pause").await;

    // Select a one-second countdown using the actual touch controls.
    for _ in 0..5 {
        click(manager, Some(3), "Minute -").await;
    }
    click(manager, Some(3), "Start").await;
    wait_label(manager, "Timer finished!", true).await;
    click(manager, None, "Dismiss").await;

    // Keep the saved occurrence well ahead of now, including around midnight.
    let current_hour = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 3600
        % 24;
    let alarm_hour = (current_hour + 12) % 24;
    for _ in 0..(alarm_hour + 24 - 7) % 24 {
        click(manager, Some(1), "Hour +").await;
    }
    click(manager, Some(1), "Save and enable").await;
    wait_label(manager, &format!("{alarm_hour:02}:00 UTC\nEnabled:"), true).await;
    let mut saved = Vec::new();
    xila::virtual_file_system::File::read_from_path(
        context.virtual_file_system,
        task,
        "/users/1000/applications/clock/configuration/settings.1",
        &mut saved,
    )
    .await
    .unwrap();
    assert_eq!(saved.len(), 38, "real WASI persistence record");

    close_and_join(manager, handle).await;
    let reopened = xila::executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/applications/executables/clock".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    wait_label(manager, "Cancel", true).await;
    // No save action: the new guest must load the existing enabled alarm.
    click(manager, Some(1), "Hour +").await;
    wait_label(
        manager,
        &format!(
            "{:02}:00 UTC\nEnabled: {alarm_hour:02}:00 UTC",
            (alarm_hour + 1) % 24
        ),
        true,
    )
    .await;
    close_and_join(manager, reopened).await;
}

async fn close_and_join(manager: &Manager, handle: xila::task::JoinHandle<isize>) {
    let root = manager.get_window_identifier(0).await.unwrap();
    manager.send_window_close_request(root).await.unwrap();
    let mut join = core::pin::pin!(handle.join());
    for _ in 0..1000 {
        if let Poll::Ready(status) =
            poll_fn(|context| Poll::Ready(join.as_mut().poll(context))).await
        {
            assert_eq!(status, 0);
            assert_eq!(manager.get_window_count().await.unwrap(), 0);
            return;
        }
        xila::task::sleep(Duration::from_millis(10)).await;
    }
    panic!("Clock did not exit on close request");
}
