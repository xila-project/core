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
