#![cfg(all(feature = "host", feature = "graphics"))]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
#[path = "support/graphics.rs"]
mod support;

use wasm::{
    graphics::{Function, GraphicsCall, GraphicsError, GraphicsState, dispatch},
    translation::{GuestMemory, GuestPointer, GuestSlice, GuestValue, WasmUsize},
};
use xila::graphics::lvgl;

fn call(
    state: &mut GraphicsState,
    memory: &mut GuestMemory<'_>,
    function: Function,
    arguments: &[WasmUsize],
    result: WasmUsize,
) -> Result<(), GraphicsError> {
    let mut words = [0; 7];
    words[..arguments.len()].copy_from_slice(arguments);
    dispatch(
        state,
        memory,
        GraphicsCall {
            function: function.id(),
            arguments: words,
            count: arguments.len() as i32,
            result,
        },
    )
}
fn handle(memory: &GuestMemory<'_>) -> WasmUsize {
    memory.read(GuestPointer::new(1)).unwrap()
}

#[xila::task::test(task_path = xila::task)]
async fn checked_graphics_core_owns_objects_maps_and_events() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 4096];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut unavailable = GraphicsState::new(None);
    assert_eq!(
        call(
            &mut unavailable,
            &mut memory,
            Function::WindowCreate,
            &[],
            1
        ),
        Err(GraphicsError::Unavailable)
    );
    let mut state = GraphicsState::new(Some(manager));
    assert_eq!(
        dispatch(
            &mut state,
            &mut memory,
            GraphicsCall {
                function: -1,
                arguments: [0; 7],
                count: 0,
                result: 0
            }
        ),
        Err(GraphicsError::Unsupported)
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::WindowCreate, &[0], 1),
        Err(GraphicsError::InvalidArgumentsCount)
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::WindowCreate, &[], 0),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    let window_pointer = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    call(
        &mut state,
        &mut memory,
        Function::ObjectCreate,
        &[window],
        1,
    )
    .unwrap();
    let container = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::SliderCreate,
        &[container],
        1,
    )
    .unwrap();
    let slider = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::SliderSetRange,
        &[slider, 0, 100],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SliderSetValue,
        &[slider, 37, 0],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SliderGetValue,
        &[slider],
        68,
    )
    .unwrap();
    assert_eq!(memory.read::<i32>(GuestPointer::new(68)).unwrap(), 37);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::SliderSetRange,
            &[slider, 100, 0],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::SliderGetValue, &[0], 68),
        Err(GraphicsError::InvalidHandle)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeVariant,
        &[container, 2],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[container, 5],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeError,
        &[container, 1],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeError,
        &[container, 0],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeError,
            &[container, 2],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::LabelCreate,
        &[container],
        1,
    )
    .unwrap();
    let label = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetWidth,
        &[container, 123],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectGetContentWidth,
        &[container],
        64,
    )
    .unwrap();
    assert!(memory.read::<i32>(GuestPointer::new(64)).unwrap() > 0);
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetBackgroundRgb,
        &[container, 0x123456],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetTextRgb,
        &[container, 0xabcdef],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetBackgroundRgb,
            &[container, 0x0100_0000],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            let container_pointer = lvgl::lv_obj_get_child(window_pointer, 0);
            let background = lvgl::lv_obj_get_style_bg_color(container_pointer, lvgl::LV_PART_MAIN);
            let text = lvgl::lv_obj_get_style_text_color(container_pointer, lvgl::LV_PART_MAIN);
            assert_eq!(lvgl::lv_color_to_u32(background) & 0x00ff_ffff, 0x123456);
            assert_eq!(lvgl::lv_color_to_u32(text) & 0x00ff_ffff, 0xabcdef);
        }
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ButtonmatrixGetSelectedButton,
            &[label],
            1
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::ObjectCreate, &[0], 1),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetWidth,
            &[999, 1],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    let mut other = GraphicsState::new(Some(manager));
    assert_eq!(
        call(
            &mut other,
            &mut memory,
            Function::ObjectSetWidth,
            &[window, 1],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    for (function, args) in [
        (Function::ObjectSetFlexFlow, vec![window, 1]),
        (Function::ObjectSetFlexGrow, vec![container, 1]),
        (Function::ObjectSetHeight, vec![container, 40]),
        (Function::ObjectSetWidth, vec![container, 200]),
        (
            Function::ObjectAddFlag,
            vec![
                container,
                lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE as WasmUsize,
            ],
        ),
    ] {
        call(&mut state, &mut memory, function, &args, 0).unwrap();
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetFlexFlow,
            &[window, 3],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    for (i, byte) in b"Hi\0".iter().enumerate() {
        memory
            .write(GuestPointer::new(300 + i as WasmUsize), *byte)
            .unwrap();
    }
    call(
        &mut state,
        &mut memory,
        Function::WindowSetIcon,
        &[window, 300, 0x112233],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::LabelSetText,
        &[label, 300],
        0,
    )
    .unwrap();
    call(&mut state, &mut memory, Function::Percentage, &[100], 1).unwrap();
    assert_eq!(
        memory.read(GuestPointer::<i32>::new(1)),
        Some(unsafe { lvgl::lv_pct(100) })
    );

    call(
        &mut state,
        &mut memory,
        Function::ButtonmatrixCreate,
        &[window],
        1,
    )
    .unwrap();
    let matrix = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectAddFlag,
        &[
            matrix,
            lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE as WasmUsize,
        ],
        0,
    )
    .unwrap();
    memory
        .write(GuestPointer::new(400), 300 as WasmUsize)
        .unwrap();
    memory
        .write(
            GuestPointer::new(400 + WasmUsize::SIZE as WasmUsize),
            0 as WasmUsize,
        )
        .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ButtonmatrixSetMap,
        &[matrix, 400],
        0,
    )
    .unwrap();
    // Replacing retained storage must switch LVGL before dropping the old map.
    call(
        &mut state,
        &mut memory,
        Function::ButtonmatrixSetMap,
        &[matrix, 400],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ButtonmatrixSetMap,
            &[matrix, 4095],
            0
        ),
        Err(GraphicsError::InvalidMemory)
    );
    call(
        &mut state,
        &mut memory,
        Function::ButtonmatrixGetSelectedButton,
        &[matrix],
        1,
    )
    .unwrap();

    let _lock = manager.synchronous_lock();
    let matrix_pointer = unsafe { lvgl::lv_obj_get_child(window_pointer, 1) };
    unsafe {
        lvgl::lv_obj_send_event(
            matrix_pointer,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    }
    drop(_lock);
    // Invalid outputs leave the event queue intact.
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::WindowPopEvent,
            &[window, 100, 4095],
            0
        ),
        Err(GraphicsError::InvalidMemory)
    );
    let mut clicked = false;
    for _ in 0..32 {
        call(
            &mut state,
            &mut memory,
            Function::WindowPopEvent,
            &[window, 101, 107],
            0,
        )
        .unwrap();
        let code = memory.read(GuestPointer::<u32>::new(101)).unwrap();
        if code == lvgl::lv_event_code_t_LV_EVENT_CLICKED {
            assert_eq!(
                memory.read(GuestPointer::<WasmUsize>::new(107)),
                Some(matrix)
            );
            clicked = true;
        }
        if code == 0 {
            break;
        }
    }
    assert!(clicked);
    call(
        &mut state,
        &mut memory,
        Function::WindowPopEvent,
        &[window, 101, 101],
        0,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<WasmUsize>::new(101)), Some(0));

    call(
        &mut state,
        &mut memory,
        Function::ObjectDelete,
        &[container],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LabelSetText,
            &[label, 300],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectCreate,
        &[window],
        1,
    )
    .unwrap();
    assert!(handle(&memory) > matrix); // Deleted IDs are never reused.
    {
        let _lock = manager.synchronous_lock();
        unsafe {
            lvgl::lv_obj_send_event(
                matrix_pointer,
                lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                core::ptr::null_mut(),
            );
            lvgl::lv_obj_delete(matrix_pointer);
        }
    }
    // A stale queued event must not become a capability to recycled native storage.
    for _ in 0..32 {
        call(
            &mut state,
            &mut memory,
            Function::WindowPopEvent,
            &[window, 101, 107],
            0,
        )
        .unwrap();
        assert_ne!(
            memory.read(GuestPointer::<WasmUsize>::new(107)),
            Some(matrix)
        );
        if memory.read(GuestPointer::<u32>::new(101)) == Some(0) {
            break;
        }
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ButtonmatrixGetSelectedButton,
            &[matrix],
            1
        ),
        Err(GraphicsError::InvalidHandle)
    );
    for _ in 6..256 {
        call(
            &mut state,
            &mut memory,
            Function::ObjectCreate,
            &[window],
            1,
        )
        .unwrap();
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectCreate,
            &[window],
            1
        ),
        Err(GraphicsError::ResourceLimit)
    );
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
    // Unknown native targets have no registry DELETE callback. Reusing their
    // address must not turn an already queued click into a newly issued handle.
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let root = handle(&memory);
    let root_pointer = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    let old_pointer = {
        let _lock = manager.synchronous_lock();
        unsafe {
            let unknown = lvgl::lv_obj_create(root_pointer);
            lvgl::lv_obj_add_flag(unknown, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
            lvgl::lv_obj_send_event(
                unknown,
                lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                core::ptr::null_mut(),
            );
            lvgl::lv_obj_delete(unknown);
            unknown
        }
    };
    let mut recycled = false;
    for _ in 0..200 {
        call(&mut state, &mut memory, Function::ObjectCreate, &[root], 1).unwrap();
        let new_handle = handle(&memory);
        let pointer = {
            let _lock = manager.synchronous_lock();
            unsafe { lvgl::lv_obj_get_child(root_pointer, 0) }
        };
        if pointer == old_pointer {
            recycled = true;
            let mut old_click = false;
            for _ in 0..1024 {
                call(
                    &mut state,
                    &mut memory,
                    Function::WindowPopEvent,
                    &[root, 101, 107],
                    0,
                )
                .unwrap();
                let code = memory.read(GuestPointer::<u32>::new(101)).unwrap();
                if code == lvgl::lv_event_code_t_LV_EVENT_CLICKED {
                    old_click = true;
                    assert_eq!(
                        memory.read(GuestPointer::<WasmUsize>::new(107)),
                        Some(0),
                        "old native click acquired new handle {new_handle}"
                    );
                }
                if code == 0 {
                    break;
                }
            }
            assert!(old_click, "preserve unknown events with a null target");
            break;
        }
        call(
            &mut state,
            &mut memory,
            Function::ObjectDelete,
            &[new_handle],
            0,
        )
        .unwrap();
    }
    assert!(recycled, "test must actually exercise native address reuse");
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let root = handle(&memory);
    call(&mut state, &mut memory, Function::LabelCreate, &[root], 1).unwrap();
    let label = handle(&memory);
    let pointer = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        unsafe { lvgl::lv_obj_delete(pointer) };
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LabelSetText,
            &[label, 300],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);

    // Clock widgets keep the same handle/type and borrowed-memory boundary.
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let root = handle(&memory);
    call(&mut state, &mut memory, Function::ButtonCreate, &[root], 1).unwrap();
    let button = handle(&memory);
    // The shared polish contract uses closed variants and bounded sliders.
    for variant in 0..5 {
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[button, variant],
            0,
        )
        .unwrap();
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[button, 5],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[root, 0],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[999, 0],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::SliderCreate, &[999], 1),
        Err(GraphicsError::InvalidHandle)
    );
    for output in [0, 4095] {
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::SliderCreate,
                &[root],
                output
            ),
            Err(GraphicsError::InvalidMemory)
        );
    }
    call(&mut state, &mut memory, Function::SliderCreate, &[root], 1).unwrap();
    let slider = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::SliderSetRange,
        &[slider, 0, 100],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SliderSetValue,
        &[slider, 37, 0],
        0,
    )
    .unwrap();
    for (min, max) in [(100, 0), (10, 10), (0, 100_001), (WasmUsize::MAX, 100)] {
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::SliderSetRange,
                &[slider, min, max],
                0
            ),
            Err(GraphicsError::InvalidValue)
        );
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::SliderSetValue,
            &[slider, WasmUsize::MAX, 0],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    for object in [0, 999, button] {
        for (function, args, output) in [
            (Function::SliderSetRange, vec![object, 0, 100], 0),
            (Function::SliderSetValue, vec![object, 37, 0], 0),
            (Function::SliderGetValue, vec![object], 64),
        ] {
            assert_eq!(
                call(&mut state, &mut memory, function, &args, output),
                Err(GraphicsError::InvalidHandle)
            );
        }
    }
    for output in [0, 4095] {
        assert_eq!(
            call(
                &mut state,
                &mut memory,
                Function::SliderGetValue,
                &[slider],
                output
            ),
            Err(GraphicsError::InvalidMemory)
        );
    }
    call(
        &mut state,
        &mut memory,
        Function::SliderGetValue,
        &[slider],
        64,
    )
    .unwrap();
    assert_eq!(memory.read::<i32>(GuestPointer::new(64)), Some(37));
    call(
        &mut state,
        &mut memory,
        Function::ObjectDelete,
        &[slider],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::SliderGetValue,
            &[slider],
            64
        ),
        Err(GraphicsError::InvalidHandle)
    );
    call(
        &mut state,
        &mut memory,
        Function::CheckboxCreate,
        &[root],
        1,
    )
    .unwrap();
    let checkbox = handle(&memory);
    memory
        .bytes_mut(GuestSlice::new(300, 6))
        .unwrap()
        .copy_from_slice(b"Clock\0");
    call(
        &mut state,
        &mut memory,
        Function::CheckboxSetText,
        &[checkbox, 300],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CheckboxSetText,
            &[button, 300],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CheckboxSetText,
            &[checkbox, 4096],
            0
        ),
        Err(GraphicsError::InvalidMemory)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectAddState,
        &[checkbox, lvgl::lv_state_t_LV_STATE_CHECKED as WasmUsize],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectGetState,
        &[checkbox],
        1,
    )
    .unwrap();
    assert_eq!(
        memory.read(GuestPointer::<u32>::new(1)).unwrap() & lvgl::lv_state_t_LV_STATE_CHECKED,
        lvgl::lv_state_t_LV_STATE_CHECKED
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectAddState,
            &[checkbox, 1 << 31],
            0
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(&mut state, &mut memory, Function::TabviewCreate, &[root], 1).unwrap();
    let tabs = handle(&memory);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TabviewGetTabActive,
            &[button],
            1
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TabviewAddTab,
            &[checkbox, 300],
            1
        ),
        Err(GraphicsError::InvalidHandle)
    );
    call(
        &mut state,
        &mut memory,
        Function::TabviewAddTab,
        &[tabs, 300],
        1,
    )
    .unwrap();
    let tab = handle(&memory);
    assert_ne!(tab, tabs);
    call(
        &mut state,
        &mut memory,
        Function::TabviewGetTabActive,
        &[tabs],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<u32>::new(1)), Some(0));
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}
