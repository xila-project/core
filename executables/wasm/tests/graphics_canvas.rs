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
            result: 1,
        },
    )?;
    Ok(memory.read(GuestPointer::new(1)).unwrap_or(0))
}

#[xila::task::test(task_path = xila::task)]
async fn indexed_canvas_and_text_entry_are_checked_and_reclaimed() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut state = GraphicsState::new(Some(manager));
    let mut bytes = [0; 1024];
    let mut memory = GuestMemory::new(&mut bytes);
    let root = call(&mut state, &mut memory, Function::WindowCreate, &[]).unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasCreate,
            &[root, 0, 100]
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasCreate,
            &[root, 300, 300]
        ),
        Err(GraphicsError::InvalidValue)
    );
    let canvas = call(
        &mut state,
        &mut memory,
        Function::IndexedCanvasCreate,
        &[root, 2, 2],
    )
    .unwrap();
    memory.write(GuestPointer::new(100), 0x01000100u32).unwrap();
    memory.write(GuestPointer::new(200), 0xffffffu32).unwrap();
    memory.write(GuestPointer::new(204), 0xff0000u32).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::IndexedCanvasSetPixels,
        &[canvas, 100, 4, 200, 2],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasSetPixels,
            &[root, 100, 4, 200, 2]
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasSetPixels,
            &[canvas, 1022, 4, 200, 2]
        ),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasSetPixels,
            &[canvas, 100, 3, 200, 2]
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasSetPixels,
            &[canvas, 100, 4, 200, 1]
        ),
        Err(GraphicsError::InvalidValue)
    );
    let textarea = call(&mut state, &mut memory, Function::TextareaCreate, &[root]).unwrap();
    memory.write(GuestPointer::new(300), 0x00636261u32).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::TextareaSetText,
        &[textarea, 300],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TextareaGetText,
            &[textarea, 400, 3]
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::TextareaGetText,
        &[textarea, 400, 4],
    )
    .unwrap();
    assert_eq!(memory.read::<u32>(GuestPointer::new(400)), Some(0x00636261));
    let keyboard = call(&mut state, &mut memory, Function::KeyboardCreate, &[root]).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::KeyboardSetTextarea,
        &[keyboard, textarea],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::KeyboardSetTextarea,
            &[keyboard, canvas]
        ),
        Err(GraphicsError::InvalidHandle)
    );
    // Validate all pointer outputs before removing an event from the queue.
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::WindowPopPointerEvent,
            &[root, 500, 520, 540, 560, 580, 1023]
        ),
        Err(GraphicsError::InvalidMemory)
    );
    let native = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            lvgl::lv_obj_send_event(
                native,
                lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
                core::ptr::null_mut(),
            );
        }
    }
    for _ in 0..128 {
        call(
            &mut state,
            &mut memory,
            Function::WindowPopPointerEvent,
            &[root, 500, 520, 540, 560, 580, 600],
        )
        .unwrap();
        if memory.read::<u32>(GuestPointer::new(500)) == Some(wasm::EventCode::ValueChanged as u32)
        {
            break;
        }
    }
    assert_eq!(memory.read::<i32>(GuestPointer::new(540)), Some(i32::MIN));
    assert_eq!(memory.read::<i32>(GuestPointer::new(580)), Some(0));
    let large = call(
        &mut state,
        &mut memory,
        Function::IndexedCanvasCreate,
        &[root, 255, 256],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasCreate,
            &[root, 16, 16]
        ),
        Err(GraphicsError::ResourceLimit)
    );
    call(&mut state, &mut memory, Function::ObjectDelete, &[large]).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::IndexedCanvasCreate,
        &[root, 16, 16],
    )
    .unwrap();
    call(&mut state, &mut memory, Function::ObjectDelete, &[root]).unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::IndexedCanvasSetPixels,
            &[canvas, 100, 4, 200, 2]
        ),
        Err(GraphicsError::InvalidHandle)
    );
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}
