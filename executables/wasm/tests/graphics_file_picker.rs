#![cfg(all(feature = "host", feature = "graphics"))]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
#[path = "support/graphics.rs"]
mod support;

use wasm::{
    graphics::{Function, GraphicsCall, GraphicsError, GraphicsState, dispatch},
    translation::{GuestMemory, GuestPointer, WasmUsize},
};
use xila::graphics::lvgl;

fn call(
    state: &mut GraphicsState,
    memory: &mut GuestMemory<'_>,
    function: Function,
    args: &[WasmUsize],
) -> Result<WasmUsize, GraphicsError> {
    let mut arguments = [0; 7];
    arguments[..args.len()].copy_from_slice(args);
    dispatch(
        state,
        memory,
        GraphicsCall {
            function: function.id(),
            arguments,
            count: args.len() as i32,
            result: 8,
        },
    )?;
    Ok(memory.read(GuestPointer::new(8)).unwrap_or(0))
}

#[xila::task::test(task_path = xila::task)]
async fn picker_validates_modes_handles_memory_and_parent_lifetime() {
    let standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let (keypad, original_group) = {
        let _guard = manager.synchronous_lock();
        unsafe {
            let mut input = lvgl::lv_indev_get_next(core::ptr::null_mut());
            while !input.is_null()
                && lvgl::lv_indev_get_type(input) != lvgl::lv_indev_type_t_LV_INDEV_TYPE_KEYPAD
            {
                input = lvgl::lv_indev_get_next(input);
            }
            assert!(!input.is_null());
            (input, lvgl::lv_indev_get_group(input))
        }
    };
    let task = standard
        .context
        .task_manager
        .get_current_task_identifier()
        .await;
    let mut state =
        GraphicsState::with_file_system(Some(manager), standard.context.virtual_file_system, task);
    let mut bytes = [0; 4096];
    bytes[100..102].copy_from_slice(b"/\0");
    bytes[120..129].copy_from_slice(b"relative\0");
    bytes[140..142].copy_from_slice(&[0xff, 0]);
    let mut memory = GuestMemory::new(&mut bytes);
    let root = call(&mut state, &mut memory, Function::WindowCreate, &[]).unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerCreate,
            &[root, 99, 100]
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerCreate,
            &[0, 0, 100]
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerCreate,
            &[root, 0, 4096]
        ),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerCreate,
            &[root, 0, 120]
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerCreate,
            &[root, 0, 140]
        ),
        Err(GraphicsError::InvalidValue)
    );
    let picker = call(
        &mut state,
        &mut memory,
        Function::FilePickerCreate,
        &[root, 2, 100],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetStatus,
            &[picker]
        ),
        Ok(0)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetStatus,
            &[root]
        ),
        Err(GraphicsError::InvalidHandle)
    );
    call(
        &mut state,
        &mut memory,
        Function::FilePickerGetPath,
        &[picker, 200, 1024],
    )
    .unwrap();
    assert_eq!(memory.read::<i32>(GuestPointer::new(8)), Some(-1));
    let native = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _guard = manager.synchronous_lock();
        unsafe {
            // Escape is handled by the component, and remains distinguishable
            // from a still-pending picker with no result.
            let picker_object = lvgl::lv_obj_get_child(native, -1);
            let mut key = lvgl::lv_key_t_LV_KEY_ESC as u32;
            lvgl::lv_obj_send_event(
                picker_object,
                lvgl::lv_event_code_t_LV_EVENT_KEY,
                (&mut key as *mut u32).cast(),
            );
        }
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetStatus,
            &[picker]
        ),
        Ok(2)
    );
    let selected = call(
        &mut state,
        &mut memory,
        Function::FilePickerCreate,
        &[root, 2, 100],
    )
    .unwrap();
    {
        let _guard = manager.synchronous_lock();
        unsafe {
            let object = lvgl::lv_obj_get_child(native, -1);
            let path = lvgl::lv_obj_get_child(object, 0);
            lvgl::lv_obj_send_event(
                path,
                lvgl::lv_event_code_t_LV_EVENT_READY,
                core::ptr::null_mut(),
            );
        }
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetStatus,
            &[selected]
        ),
        Ok(1)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetPath,
            &[selected, 200, 1]
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetPath,
            &[selected, 4095, 2]
        ),
        Err(GraphicsError::InvalidMemory)
    );
    call(
        &mut state,
        &mut memory,
        Function::FilePickerGetPath,
        &[selected, 200, 2],
    )
    .unwrap();
    assert_eq!(memory.read::<i32>(GuestPointer::new(8)), Some(1));
    assert_eq!(
        memory
            .bytes(wasm::translation::GuestSlice::new(200, 2))
            .unwrap(),
        b"/\0"
    );
    // Copying is repeatable; short or invalid buffers never consume the result.
    call(
        &mut state,
        &mut memory,
        Function::FilePickerGetPath,
        &[selected, 202, 1024],
    )
    .unwrap();
    assert_eq!(memory.read::<i32>(GuestPointer::new(8)), Some(1));
    for (arguments, expected) in [
        ([root, 2, 100], GraphicsError::InvalidHandle),
        ([selected, 99, 100], GraphicsError::InvalidValue),
        ([selected, 2, 4096], GraphicsError::InvalidMemory),
        ([selected, 2, 120], GraphicsError::InvalidValue),
        ([selected, 2, 140], GraphicsError::InvalidValue),
    ] {
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::FilePickerReset,
                &arguments
            ),
            Err(expected)
        );
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::FilePickerGetStatus,
                &[selected]
            ),
            Ok(1)
        );
    }
    for (mode, keyboard_ready, expected_status) in [(0, true, 0), (2, true, 1), (0, false, 0)] {
        call(
            &mut state,
            &mut memory,
            Function::FilePickerReset,
            &[selected, mode, 100],
        )
        .unwrap();
        {
            let _guard = manager.synchronous_lock();
            unsafe {
                let object = lvgl::lv_obj_get_child(native, -1);
                let target = if keyboard_ready {
                    lvgl::lv_obj_get_child(object, 4)
                } else {
                    lvgl::lv_obj_get_child(lvgl::lv_obj_get_child(object, 1), 3)
                };
                lvgl::lv_obj_send_event(
                    target,
                    if keyboard_ready {
                        lvgl::lv_event_code_t_LV_EVENT_READY
                    } else {
                        lvgl::lv_event_code_t_LV_EVENT_CLICKED
                    },
                    core::ptr::null_mut(),
                );
            }
        }
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::FilePickerGetStatus,
                &[selected]
            ),
            Ok(expected_status)
        );
    }
    // A retained picker must survive more sessions than the monotonic handle
    // registry's total capacity without registering another object.
    for cycle in 0..300 {
        dispatch(
            &mut state,
            &mut memory,
            GraphicsCall {
                function: 0xf030,
                arguments: [selected, (cycle % 3) as WasmUsize, 100, 0, 0, 0, 0],
                count: 3,
                result: 0,
            },
        )
        .unwrap();
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::FilePickerGetStatus,
                &[selected]
            ),
            Ok(0)
        );
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetPath,
            &[selected, 200, 1024],
        )
        .unwrap();
        assert_eq!(memory.read::<i32>(GuestPointer::new(8)), Some(-1));
        {
            let _guard = manager.synchronous_lock();
            unsafe {
                let object = lvgl::lv_obj_get_child(native, -1);
                assert!(!lvgl::lv_obj_has_flag(
                    object,
                    lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN
                ));
                assert_ne!(lvgl::lv_indev_get_group(keypad), original_group);
                let mut key = lvgl::lv_key_t_LV_KEY_ESC as u32;
                lvgl::lv_obj_send_event(
                    object,
                    lvgl::lv_event_code_t_LV_EVENT_KEY,
                    (&mut key as *mut u32).cast(),
                );
                assert_eq!(lvgl::lv_indev_get_group(keypad), original_group);
            }
        }
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::FilePickerGetStatus,
                &[selected]
            ),
            Ok(2)
        );
    }
    // An oversized row must not be truncated by LVGL into a different existing
    // prefix filename. Build a real VFS directory with both names present.
    let vfs = standard.context.virtual_file_system;
    let mut directory = String::new();
    for length in [250, 250, 250, 200] {
        directory.push('/');
        directory.push_str(&"d".repeat(length));
        vfs.create_directory(task, &directory.as_str())
            .await
            .unwrap();
    }
    let full_name = "x".repeat(100);
    let prefix_name = "x".repeat(1023 - directory.len() - 1);
    for name in [&full_name, &prefix_name] {
        use xila::file_system::{AccessFlags, CreateFlags, Flags};
        xila::virtual_file_system::File::open(
            vfs,
            task,
            format!("{directory}/{name}").as_str(),
            Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
        )
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();
    }
    memory
        .bytes_mut(wasm::translation::GuestSlice::new(
            1500,
            (directory.len() + 1) as WasmUsize,
        ))
        .unwrap()[..directory.len()]
        .copy_from_slice(directory.as_bytes());
    // Guest memory is zero initialized, including the terminating byte.
    call(
        &mut state,
        &mut memory,
        Function::FilePickerReset,
        &[selected, 0, 1500],
    )
    .unwrap();
    {
        let _guard = manager.synchronous_lock();
        unsafe {
            let object = lvgl::lv_obj_get_child(native, -1);
            let path = lvgl::lv_obj_get_child(object, 0);
            let list = lvgl::lv_obj_get_child(object, 3);
            let mut row = core::ptr::null_mut();
            for index in 0..16 {
                let candidate = lvgl::lv_obj_get_child(list, index);
                let label = lvgl::lv_obj_get_child(candidate, 0);
                if std::ffi::CStr::from_ptr(lvgl::lv_label_get_text(label)).to_bytes()
                    == full_name.as_bytes()
                {
                    row = candidate;
                    break;
                }
            }
            assert!(
                !row.is_null(),
                "oversized entry must be present in bounded listing"
            );
            lvgl::lv_obj_send_event(
                row,
                lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                core::ptr::null_mut(),
            );
            assert_eq!(
                std::ffi::CStr::from_ptr(lvgl::lv_textarea_get_text(path))
                    .to_str()
                    .unwrap(),
                directory,
                "invalid row must leave path untouched"
            );
            lvgl::lv_obj_send_event(
                path,
                lvgl::lv_event_code_t_LV_EVENT_READY,
                core::ptr::null_mut(),
            );
        }
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetStatus,
            &[selected]
        ),
        Ok(0)
    );
    // Tear down an older pending picker while a newer modal owns the input.
    let cancel_latest = || {
        let _guard = manager.synchronous_lock();
        unsafe {
            let object = lvgl::lv_obj_get_child(native, -1);
            let mut key = lvgl::lv_key_t_LV_KEY_ESC;
            lvgl::lv_obj_send_event(
                object,
                lvgl::lv_event_code_t_LV_EVENT_KEY,
                (&mut key as *mut u32).cast(),
            );
        }
    };
    cancel_latest();
    call(
        &mut state,
        &mut memory,
        Function::FilePickerReset,
        &[picker, 2, 100],
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::FilePickerReset,
        &[selected, 2, 100],
    )
    .unwrap();
    let newer_group = {
        let _guard = manager.synchronous_lock();
        unsafe { lvgl::lv_indev_get_group(keypad) }
    };
    assert_ne!(newer_group, original_group);
    call(&mut state, &mut memory, Function::ObjectDelete, &[picker]).unwrap();
    {
        let _guard = manager.synchronous_lock();
        assert_eq!(unsafe { lvgl::lv_indev_get_group(keypad) }, newer_group);
    }
    cancel_latest();
    {
        let _guard = manager.synchronous_lock();
        assert_eq!(unsafe { lvgl::lv_indev_get_group(keypad) }, original_group);
    }
    call(&mut state, &mut memory, Function::ObjectDelete, &[root]).unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::FilePickerGetStatus,
            &[picker]
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
    {
        let _guard = manager.synchronous_lock();
        // Two retained completed pickers are deleted in creation order; the
        // keyboard must return to its live application group, not a stale group.
        assert_eq!(unsafe { lvgl::lv_indev_get_group(keypad) }, original_group);
    }
}
