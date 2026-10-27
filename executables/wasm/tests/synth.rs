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
    ffi::CStr,
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};
use xila::graphics::{Manager, lvgl};

unsafe fn label(root: *mut lvgl::lv_obj_t, text: &str) -> *mut lvgl::lv_obj_t {
    unsafe {
        if lvgl::lv_obj_has_class(root, &lvgl::lv_label_class)
            && CStr::from_ptr(lvgl::lv_label_get_text(root))
                .to_bytes()
                .starts_with(text.as_bytes())
        {
            return root;
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let found = label(lvgl::lv_obj_get_child(root, index as i32), text);
            if !found.is_null() {
                return found;
            }
        }
        core::ptr::null_mut()
    }
}

async fn wait_label(manager: &Manager, text: &str) {
    for _ in 0..400 {
        if manager.get_window_count().await.unwrap() != 0 {
            let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
            let _lock = manager.synchronous_lock();
            if unsafe { !label(root, text).is_null() } {
                return;
            }
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Synth did not display {text:?}");
}

async fn event(manager: &Manager, text: &str, code: u32) {
    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let _lock = manager.synchronous_lock();
    unsafe {
        let found = label(root, text);
        assert!(!found.is_null(), "missing Synth control {text:?}");
        lvgl::lv_obj_send_event(lvgl::lv_obj_get_parent(found), code, core::ptr::null_mut());
    }
}

#[xila::task::test(task_path=xila::task)]
async fn real_synth_guest_recovers_from_silent_mode_plays_releases_and_closes() {
    let mut standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let context = support::context(&standard, Some(manager));
    standard.context = context;
    let task = context.task_manager.get_current_task_identifier().await;
    let binary = applications::build("xila-synth").unwrap();
    drivers_std::loader::load_to_virtual_file_system(
        context.task_manager,
        context.virtual_file_system,
        binary,
        "/applications/executables/synth",
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
        vec!["/applications/executables/synth".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    wait_label(manager, "Silent mode").await;
    // A real pointer press still provides feedback without an audio device.
    event(manager, "C", lvgl::lv_event_code_t_LV_EVENT_PRESSED).await;
    wait_label(manager, "C *").await;
    event(manager, "C *", lvgl::lv_event_code_t_LV_EVENT_PRESS_LOST).await;

    let system: &'static audio::AudioSystem =
        Box::leak(Box::new(audio::AudioSystem::new().unwrap()));
    // SAFETY: this leaked system never moves and outlives all device streams.
    let system = unsafe { core::pin::Pin::new_unchecked(system) };
    let mut output = system.open_output().unwrap();
    context
        .virtual_file_system
        .mount_character_device(
            task,
            "/devices/audio/playback",
            audio::AudioStreamDevice::new(system),
        )
        .await
        .unwrap();
    event(
        manager,
        "Retry audio",
        lvgl::lv_event_code_t_LV_EVENT_CLICKED,
    )
    .await;
    wait_label(manager, "Audio ready").await;
    event(manager, "Sound", lvgl::lv_event_code_t_LV_EVENT_CLICKED).await;
    wait_label(manager, "Octave 4 | Square").await;
    event(manager, "Oct +", lvgl::lv_event_code_t_LV_EVENT_CLICKED).await;
    wait_label(manager, "Octave 5 | Square").await;
    event(manager, "C", lvgl::lv_event_code_t_LV_EVENT_PRESSED).await;
    wait_label(manager, "C *").await;
    let mut heard = false;
    let mut bytes = [0u8; 1024];
    for _ in 0..100 {
        system.mix_next_block().unwrap();
        let count = output.with_consumer(|consumer| consumer.read(&mut bytes));
        heard |= bytes[..count].iter().any(|byte| *byte != 0);
        xila::task::sleep(Duration::from_millis(3)).await;
    }
    assert!(heard, "held note must reach the real VFS/PCM mixer");
    event(manager, "C *", lvgl::lv_event_code_t_LV_EVENT_RELEASED).await;
    for _ in 0..150 {
        system.mix_next_block().unwrap();
        output.with_consumer(|consumer| consumer.read(&mut bytes));
        xila::task::sleep(Duration::from_millis(3)).await;
    }
    assert!(
        bytes.iter().all(|byte| *byte == 0),
        "released note must become silent"
    );
    event(manager, "Vol -", lvgl::lv_event_code_t_LV_EVENT_CLICKED).await;
    wait_label(manager, "Octave 5 | Square | Volume 60%").await;
    manager
        .send_window_close_request(manager.get_window_identifier(0).await.unwrap())
        .await
        .unwrap();
    let mut join = core::pin::pin!(handle.join());
    for _ in 0..400 {
        if let Poll::Ready(status) = poll_fn(|cx| Poll::Ready(join.as_mut().poll(cx))).await {
            assert_eq!(status, 0);
            assert_eq!(manager.get_window_count().await.unwrap(), 0);
            // Closing the guest releases its stream reservation.
            let streams: Vec<_> = (0..4).map(|_| system.open_stream().unwrap()).collect();
            drop(streams);
            return;
        }
        xila::task::sleep(Duration::from_millis(5)).await;
    }
    panic!("Synth did not exit on close request");
}
