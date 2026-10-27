#![cfg(all(
    feature = "host",
    feature = "graphics",
    feature = "memory_32",
    not(feature = "memory_64")
))]
#[path = "support/applications.rs"]
mod applications;
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
#[path = "support/graphics.rs"]
mod support;

use core::{
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};
use xila::graphics::{Manager, lvgl};

async fn wait_for_display(manager: &Manager, expected: &str) {
    for _ in 0..200 {
        if manager.get_window_count().await.unwrap() > 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let _lock = manager.synchronous_lock();
            unsafe {
                let container = lvgl::lv_obj_get_child(root, 0);
                if !container.is_null() {
                    let label = lvgl::lv_obj_get_child(container, 0);
                    if !label.is_null()
                        && core::ffi::CStr::from_ptr(lvgl::lv_label_get_text(label))
                            .to_bytes()
                            .starts_with(expected.as_bytes())
                    {
                        return;
                    }
                }
            }
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Calculator did not show {expected:?}");
}

#[xila::task::test(task_path=xila::task)]
async fn real_calculator_guest_evaluates_buttons_and_closes_on_wasmi() {
    let mut standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let context = support::context(&standard, Some(manager));
    standard.context = context;
    let task = context.task_manager.get_current_task_identifier().await;
    let binary = applications::build("xila-calculator").unwrap();
    drivers_std::loader::load_to_virtual_file_system(
        context.task_manager,
        context.virtual_file_system,
        binary,
        "/applications/executables/calculator.wasm",
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
    let handle = xila::executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/applications/executables/calculator.wasm".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    wait_for_display(manager, "0").await;
    // Native selection IDs are the real calculator map's indices, without row separators.
    for (button, expected) in [(27, "1"), (30, "1+"), (28, "1+2"), (38, "3")] {
        let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
        {
            let _lock = manager.synchronous_lock();
            unsafe {
                let matrix = lvgl::lv_obj_get_child(root, 1);
                assert!(!matrix.is_null());
                lvgl::lv_buttonmatrix_set_selected_button(matrix, button);
                lvgl::lv_obj_send_event(
                    matrix,
                    lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                    core::ptr::null_mut(),
                );
            }
        }
        wait_for_display(manager, expected).await;
    }
    let root = manager.get_window_identifier(0).await.unwrap();
    manager.send_window_close_request(root).await.unwrap();
    let mut join = core::pin::pin!(handle.join());
    for _ in 0..200 {
        let result = poll_fn(|context| Poll::Ready(join.as_mut().poll(context))).await;
        if let Poll::Ready(status) = result {
            assert_eq!(status, 0);
            assert_eq!(manager.get_window_count().await.unwrap(), 0);
            return;
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Calculator did not exit on close request");
}
