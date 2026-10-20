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
    ffi::CStr,
    future::{Future, poll_fn},
    task::Poll,
    time::Duration,
};
use xila::graphics::{Manager, lvgl};

unsafe fn label(root: *mut lvgl::lv_obj_t, text: &str, exact: bool) -> *mut lvgl::lv_obj_t {
    unsafe {
        let matches = if lvgl::lv_obj_has_class(root, &lvgl::lv_label_class) {
            let value = CStr::from_ptr(lvgl::lv_label_get_text(root)).to_bytes();
            if exact {
                value == text.as_bytes()
            } else {
                value.starts_with(text.as_bytes())
            }
        } else {
            false
        };
        if matches {
            return root;
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let found = label(lvgl::lv_obj_get_child(root, index as i32), text, exact);
            if !found.is_null() {
                return found;
            }
        }
        core::ptr::null_mut()
    }
}

unsafe fn slider(root: *mut lvgl::lv_obj_t) -> *mut lvgl::lv_obj_t {
    unsafe {
        if lvgl::lv_obj_has_class(root, &lvgl::lv_slider_class) {
            return root;
        }
        for index in 0..lvgl::lv_obj_get_child_count(root) {
            let found = slider(lvgl::lv_obj_get_child(root, index as i32));
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
            if unsafe { !label(root, text, false).is_null() } {
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
        let found = label(root, text, true);
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
    let binary = xila::executable::build_crate("synth").unwrap();
    drivers_std::loader::load_to_virtual_file_system(
        context.task_manager,
        context.virtual_file_system,
        binary,
        "/binaries/synth",
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
    let handle = xila::executable::execute(
        context,
        "/binaries/wasm",
        vec!["/binaries/synth".into()],
        standard,
        None,
    )
    .await
    .unwrap();
    wait_label(manager, "Silent mode").await;

    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let volume = slider(root);
            assert!(
                !volume.is_null(),
                "Synth should expose a continuous volume slider"
            );
            assert_eq!(lvgl::lv_slider_get_min_value(volume), 0);
            assert_eq!(lvgl::lv_slider_get_max_value(volume), 100);
            assert_eq!(lvgl::lv_slider_get_value(volume), 70);
            lvgl::lv_slider_set_value(volume, 42, false);
            lvgl::lv_obj_send_event(
                volume,
                lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
                core::ptr::null_mut(),
            );
        }
    }
    wait_label(manager, "Volume: 42%").await;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let volume = slider(root);
            lvgl::lv_slider_set_value(volume, 70, false);
            lvgl::lv_obj_send_event(
                volume,
                lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
                core::ptr::null_mut(),
            );
        }
    }
    wait_label(manager, "Volume: 70%").await;

    let root = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            lvgl::lv_obj_update_layout(root);
            let c = label(root, "C", true);
            let c_sharp = label(root, "C#", true);
            let d = label(root, "D", true);
            assert!(!c.is_null() && !c_sharp.is_null() && !d.is_null());

            let c_key = lvgl::lv_obj_get_parent(c);
            let c_sharp_key = lvgl::lv_obj_get_parent(c_sharp);
            let d_key = lvgl::lv_obj_get_parent(d);
            let mut c_area = lvgl::lv_area_t::default();
            let mut c_sharp_area = lvgl::lv_area_t::default();
            let mut d_area = lvgl::lv_area_t::default();
            lvgl::lv_obj_get_coords(c_key, &mut c_area);
            lvgl::lv_obj_get_coords(c_sharp_key, &mut c_sharp_area);
            lvgl::lv_obj_get_coords(d_key, &mut d_area);
            let c_x = c_area.x1;
            let c_sharp_x = c_sharp_area.x1;
            let d_x = d_area.x1;
            assert!(
                c_x < c_sharp_x && c_sharp_x < d_x,
                "piano key x positions should be ordered C < C# < D, got {c_x}, {c_sharp_x}, {d_x}"
            );
            assert!(lvgl::lv_obj_get_width(c_sharp_key) < lvgl::lv_obj_get_width(c_key));
            assert!(lvgl::lv_obj_get_height(c_sharp_key) < lvgl::lv_obj_get_height(c_key));
            let white = lvgl::lv_obj_get_style_bg_color(c_key, lvgl::LV_PART_MAIN);
            let black = lvgl::lv_obj_get_style_bg_color(c_sharp_key, lvgl::LV_PART_MAIN);
            assert!(!lvgl::lv_color_eq(white, black));
        }
    }

    // A real pointer press still provides feedback without an audio device.
    event(manager, "C", lvgl::lv_event_code_t_LV_EVENT_PRESSED).await;
    wait_label(manager, "C*").await;
    event(manager, "C*", lvgl::lv_event_code_t_LV_EVENT_PRESS_LOST).await;

    let system: &'static audio::AudioSystem =
        Box::leak(Box::new(audio::AudioSystem::new().unwrap()));
    // SAFETY: this leaked system never moves and outlives all device streams.
    let system = unsafe { core::pin::Pin::new_unchecked(&*system) };
    let mut output = system.open_output().unwrap();
    context
        .virtual_file_system
        .create_directory(task, &"/devices/audio")
        .await
        .unwrap();
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
    wait_label(manager, "Audio output ready").await;
    event(manager, "Waveform", lvgl::lv_event_code_t_LV_EVENT_CLICKED).await;
    wait_label(manager, "Octave 4     Square wave     70% volume").await;
    event(manager, "Octave +", lvgl::lv_event_code_t_LV_EVENT_CLICKED).await;
    wait_label(manager, "Octave 5     Square wave     70% volume").await;
    event(manager, "C", lvgl::lv_event_code_t_LV_EVENT_PRESSED).await;
    wait_label(manager, "C*").await;
    let mut heard = false;
    let mut bytes = [0u8; 1024];
    for _ in 0..100 {
        system.mix_next_block().unwrap();
        let count = output.with_consumer(|consumer| consumer.read(&mut bytes));
        heard |= bytes[..count].iter().any(|byte| *byte != 0);
        xila::task::sleep(Duration::from_millis(3)).await;
    }
    assert!(heard, "held note must reach the real VFS/PCM mixer");
    event(manager, "C*", lvgl::lv_event_code_t_LV_EVENT_RELEASED).await;
    for _ in 0..150 {
        system.mix_next_block().unwrap();
        output.with_consumer(|consumer| consumer.read(&mut bytes));
        xila::task::sleep(Duration::from_millis(3)).await;
    }
    assert!(
        bytes.iter().all(|byte| *byte == 0),
        "released note must become silent"
    );
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let volume = slider(root);
            lvgl::lv_slider_set_value(volume, 60, false);
            lvgl::lv_obj_send_event(
                volume,
                lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
                core::ptr::null_mut(),
            );
        }
    }
    wait_label(manager, "Octave 5     Square wave     60% volume").await;
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
