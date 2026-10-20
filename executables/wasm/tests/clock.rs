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

unsafe fn collect_labels(root: *mut lvgl::lv_obj_t, labels: &mut Vec<String>) {
    unsafe {
        if lvgl::lv_obj_has_class(root, &lvgl::lv_label_class) {
            labels.push(
                CStr::from_ptr(lvgl::lv_label_get_text(root))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            collect_labels(lvgl::lv_obj_get_child(root, index as i32), labels);
        }
    }
}

unsafe fn find_class(
    root: *mut lvgl::lv_obj_t,
    class: *const lvgl::lv_obj_class_t,
) -> *mut lvgl::lv_obj_t {
    unsafe {
        if lvgl::lv_obj_has_class(root, class) {
            return root;
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let found = find_class(lvgl::lv_obj_get_child(root, index as i32), class);
            if !found.is_null() {
                return found;
            }
        }
        core::ptr::null_mut()
    }
}

unsafe fn find_class_ordinal(
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
            let object = find_class_ordinal(
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

async fn wait_label(manager: &Manager, text: &str, contains: bool) {
    let mut observed = Vec::new();
    for _ in 0..1000 {
        if manager.get_window_count().await.unwrap() != 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let found = {
                let _lock = manager.synchronous_lock();
                observed.clear();
                unsafe {
                    collect_labels(root, &mut observed);
                    !find_label(root, text, contains).is_null()
                }
            };
            if found {
                return;
            }
        }
        xila::task::sleep(Duration::from_millis(10)).await;
    }
    panic!("Clock did not display {text:?}; labels: {observed:?}");
}

async fn click(manager: &Manager, tab: Option<i32>, text: &str) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let parent = if let Some(index) = tab {
            let tabs = find_class(root, &lvgl::lv_tabview_class);
            assert!(!tabs.is_null(), "Clock should contain a tab view");
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

async fn select_roller(manager: &Manager, tab: i32, ordinal: usize, value: u32) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let tabs = find_class(root, &lvgl::lv_tabview_class);
        assert!(!tabs.is_null(), "Clock should contain a tab view");
        lvgl::lv_tabview_set_active(tabs, tab as u32, false);
        let content = lvgl::lv_tabview_get_content(tabs);
        let parent = lvgl::lv_obj_get_child(content, tab);
        let mut found = 0;
        let roller = find_class_ordinal(parent, &lvgl::lv_roller_class, ordinal, &mut found);
        assert!(
            !roller.is_null(),
            "Clock should contain the requested roller"
        );
        lvgl::lv_roller_set_selected(roller, value, false);
        lvgl::lv_obj_send_event(
            roller,
            lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
            core::ptr::null_mut(),
        );
    }
}

async fn enable_alarm(manager: &Manager) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let tabs = find_class(root, &lvgl::lv_tabview_class);
        assert!(!tabs.is_null(), "Clock should contain a tab view");
        let alarm_tab = lvgl::lv_obj_get_child(lvgl::lv_tabview_get_content(tabs), 1);
        let mut found = 0;
        let switch = find_class_ordinal(alarm_tab, &lvgl::lv_switch_class, 0, &mut found);
        assert!(
            !switch.is_null(),
            "Clock should contain an alarm enable switch"
        );
        lvgl::lv_obj_add_state(switch, lvgl::lv_state_t_LV_STATE_CHECKED);
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
        "/binaries/clock",
    )
    .await
    .unwrap();
    xila::executable::mount_executables!(
        context,
        context.virtual_file_system,
        task,
        &[("/binaries/wasm", wasm::WasmExecutable)]
    )
    .await
    .unwrap();
    // Match native boot: an ordinary login user owns the application settings
    // directory, while the configuration parent stays protected.
    let administrator = context
        .users_manager
        .get_user_identifier("administrator")
        .await
        .unwrap();
    context
        .virtual_file_system
        .create_directory(task, &"/configuration/shared/clock")
        .await
        .unwrap();
    context
        .virtual_file_system
        .set_ownership(
            task,
            "/configuration/shared/clock",
            Some(administrator),
            Some(xila::users::GroupIdentifier::new(1000)),
        )
        .await
        .unwrap();
    context
        .task_manager
        .set_user(task, administrator)
        .await
        .unwrap();
    let handle = xila::executable::execute(
        context,
        "/binaries/wasm",
        vec!["/binaries/clock".into()],
        standard.duplicate().await.unwrap(),
        None,
    )
    .await
    .unwrap();
    wait_label(manager, "Cancel", true).await;
    click(manager, Some(2), "Start / pause").await;
    xila::task::sleep(Duration::from_millis(300)).await;
    click(manager, Some(2), "Lap").await;
    wait_label(manager, "1   00:00", true).await;
    click(manager, Some(2), "Start / pause").await;

    // The timer now uses hour/minute/second rollers instead of +/- buttons.
    select_roller(manager, 3, 1, 0).await;
    select_roller(manager, 3, 2, 1).await;
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
    select_roller(manager, 1, 0, alarm_hour as u32).await;
    select_roller(manager, 1, 1, 0).await;
    enable_alarm(manager).await;
    click(manager, Some(1), "Save alarm").await;
    wait_label(
        manager,
        &format!("{alarm_hour:02}:00 UTC\nAlarm enabled:"),
        true,
    )
    .await;
    let mut saved = Vec::new();
    xila::virtual_file_system::File::read_from_path(
        context.virtual_file_system,
        task,
        "/configuration/shared/clock/settings.1",
        &mut saved,
    )
    .await
    .unwrap();
    assert_eq!(saved.len(), 38, "real WASI persistence record");

    close_and_join(manager, handle).await;
    let reopened = xila::executable::execute(
        context,
        "/binaries/wasm",
        vec!["/binaries/clock".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    wait_label(manager, "Cancel", true).await;
    // No save action: the new guest must load the existing enabled alarm.
    select_roller(manager, 1, 0, ((alarm_hour + 1) % 24) as u32).await;
    select_roller(manager, 1, 1, 0).await;
    wait_label(
        manager,
        &format!(
            "{:02}:00 UTC\nAlarm enabled: {alarm_hour:02}:00 UTC",
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
