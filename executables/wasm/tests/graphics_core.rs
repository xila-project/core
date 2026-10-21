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
async fn object_position_calls_round_trip_through_the_guest_memory_abi() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 64];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectCreate,
        &[window],
        1,
    )
    .unwrap();
    let object = handle(&memory);
    let window_pointer = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;

    #[cfg(feature = "memory_64")]
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetPos,
            &[object, 1u64 << 40, 0],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetPos,
        &[object, (-17i32) as WasmUsize, 23],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectUpdateLayout,
        &[object],
        0,
    )
    .unwrap();
    {
        let _lock = manager.synchronous_lock();
        let native = unsafe { lvgl::lv_obj_get_child(window_pointer, 0) };
        assert_eq!(unsafe { lvgl::lv_obj_get_x(native) }, -17);
    }
    call(&mut state, &mut memory, Function::ObjectGetX, &[object], 1).unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(-17));
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetSize,
        &[object, 123, 45],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectUpdateLayout,
        &[object],
        0,
    )
    .unwrap();
    for (function, expected) in [
        (Function::ObjectGetWidth, 123),
        (Function::ObjectGetHeight, 45),
    ] {
        call(&mut state, &mut memory, function, &[object], 1).unwrap();
        assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(expected));
    }
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetFlexAlign,
        &[object, 0, 2, 5],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetFlexAlign,
            &[object, 6, 0, 0],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetLayout,
        &[object, 1],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectUpdateLayout,
        &[object],
        0,
    )
    .unwrap();
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}

#[xila::task::test(task_path = xila::task)]
async fn grid_layout_copies_bounded_track_arrays_from_guest_memory() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 512];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectCreate,
        &[window],
        1,
    )
    .unwrap();
    let grid = handle(&memory);
    call(&mut state, &mut memory, Function::ObjectCreate, &[grid], 1).unwrap();
    let child = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetPadding,
        &[grid, 0],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetPadding,
        &[child, 0],
        0,
    )
    .unwrap();

    call(&mut state, &mut memory, Function::GridFr, &[2], 1).unwrap();
    assert_eq!(
        memory.read(GuestPointer::<i32>::new(1)),
        Some(unsafe { lvgl::lv_grid_fr(2) })
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::GridFr, &[100], 1),
        Err(GraphicsError::InvalidValue)
    );

    for (offset, value) in [
        (200, 60),
        (204, lvgl::LV_GRID_TEMPLATE_LAST as i32),
        (220, 30),
        (224, lvgl::LV_GRID_TEMPLATE_LAST as i32),
    ] {
        memory.write(GuestPointer::new(offset), value).unwrap();
    }
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetGridDscArray,
        &[grid, 200, 220],
        0,
    )
    .unwrap();
    memory.write(GuestPointer::new(200), 10i32).unwrap();
    memory.write(GuestPointer::new(220), 10i32).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetLayout,
        &[grid, 2],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetGridAlign,
        &[grid, 3, 1],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetGridAlign,
            &[grid, 7, 0],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetSize,
        &[grid, 200, 100],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetGridCell,
        &[child, 3, 0, 1, 3, 0, 1],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectUpdateLayout,
        &[grid],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectGetWidth,
        &[child],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(60));
    call(
        &mut state,
        &mut memory,
        Function::ObjectGetHeight,
        &[child],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(30));

    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetGridDscArray,
            &[grid, 0, 220],
            0,
        ),
        Err(GraphicsError::InvalidMemory)
    );
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}

#[xila::task::test(task_path = xila::task)]
async fn style_and_font_calls_use_validated_host_tokens() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 64];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectCreate,
        &[window],
        1,
    )
    .unwrap();
    let object = handle(&memory);

    for (function, arguments) in [
        (
            Function::ObjectSetBackground,
            vec![object, 0x123456, 255, 0x80],
        ),
        (Function::ObjectSetTextColor, vec![object, 0xfedcba, 0]),
        (Function::ObjectSetBorder, vec![object, 0x334455, 2, 15, 0]),
        (Function::ObjectSetRadius, vec![object, 8, 0]),
        (Function::ObjectSetTextAlign, vec![object, 2, 0]),
        (Function::ObjectSetFont, vec![object, 2, 0]),
        (Function::ObjectSetSurface, vec![object, 1]),
        (
            Function::ObjectSetMinMaxSize,
            vec![object, 20, 200, 10, 120, 0],
        ),
    ] {
        call(&mut state, &mut memory, function, &arguments, 0).unwrap();
    }

    for (function, arguments) in [
        (
            Function::ObjectSetBackground,
            vec![object, 0x01000000, 255, 0],
        ),
        (Function::ObjectSetFont, vec![object, 6, 0]),
        (Function::ObjectSetSurface, vec![object, 4]),
        (
            Function::ObjectSetMinMaxSize,
            vec![object, 21, 20, 10, 120, 0],
        ),
    ] {
        assert_eq!(
            call(&mut state, &mut memory, function, &arguments, 0),
            Err(GraphicsError::InvalidValue)
        );
    }
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}

#[xila::task::test(task_path = xila::task)]
async fn range_controls_expose_checked_values_modes_and_orientation() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 64];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);

    for (constructor, expected) in [
        (Function::ArcCreate, 55i32),
        (Function::BarCreate, 42),
        (Function::SliderCreate, -5),
    ] {
        call(&mut state, &mut memory, constructor, &[window], 1).unwrap();
        let control = handle(&memory);
        let (minimum, maximum) = if expected < 0 { (-10, 10) } else { (0, 100) };
        call(
            &mut state,
            &mut memory,
            Function::RangeSetRange,
            &[control, minimum as WasmUsize, maximum],
            0,
        )
        .unwrap();
        call(
            &mut state,
            &mut memory,
            Function::RangeSetValue,
            &[control, expected as WasmUsize, 0],
            0,
        )
        .unwrap();
        call(
            &mut state,
            &mut memory,
            Function::RangeGetValue,
            &[control],
            1,
        )
        .unwrap();
        assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(expected));
        call(
            &mut state,
            &mut memory,
            Function::RangeSetMode,
            &[control, 2],
            0,
        )
        .unwrap();
    }

    call(&mut state, &mut memory, Function::BarCreate, &[window], 1).unwrap();
    let bar = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::RangeSetOrientation,
        &[bar, 2],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RangeSetOrientation,
            &[window, 2],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RangeSetRange,
            &[bar, 10, 10],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RangeSetMode,
            &[bar, 3],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RangeSetValue,
            &[bar, 101, 0],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );

    call(
        &mut state,
        &mut memory,
        Function::SwitchCreate,
        &[window],
        1,
    )
    .unwrap();
    let switch = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::RangeSetOrientation,
        &[switch, 2],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectAddState,
        &[switch, lvgl::lv_state_t_LV_STATE_CHECKED as WasmUsize],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectRemoveState,
        &[switch, lvgl::lv_state_t_LV_STATE_CHECKED as WasmUsize],
        0,
    )
    .unwrap();

    call(
        &mut state,
        &mut memory,
        Function::SpinnerCreate,
        &[window],
        1,
    )
    .unwrap();
    let spinner = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::SpinnerSetAnimation,
        &[spinner, 1000, 90],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinnerGetAnimationDuration,
        &[spinner],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(1000));
    call(
        &mut state,
        &mut memory,
        Function::SpinnerGetArcSweep,
        &[spinner],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<u32>::new(1)), Some(90));
    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}

#[xila::task::test(task_path = xila::task)]
async fn dropdown_and_roller_copy_bounded_options_and_report_selected_text() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 512];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);

    call(
        &mut state,
        &mut memory,
        Function::DropdownCreate,
        &[window],
        1,
    )
    .unwrap();
    let dropdown = handle(&memory);
    memory
        .bytes_mut(GuestSlice::new(64, 17))
        .unwrap()
        .copy_from_slice(b"Alpha\nBeta\nGamma\0");
    call(
        &mut state,
        &mut memory,
        Function::DropdownSetOptions,
        &[dropdown, 64],
        0,
    )
    .unwrap();
    memory
        .bytes_mut(GuestSlice::new(64, 17))
        .unwrap()
        .fill(b'X');
    call(
        &mut state,
        &mut memory,
        Function::DropdownSetSelected,
        &[dropdown, 1],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::DropdownSetSelectedHighlight,
        &[dropdown, 1],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::DropdownGetHighlight,
        &[dropdown],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<u32>::new(1)), Some(1));
    call(
        &mut state,
        &mut memory,
        Function::DropdownGetOptionCount,
        &[dropdown],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<u32>::new(1)), Some(3));
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::DropdownGetSelectedString,
            &[dropdown, 1, 32],
            1,
        ),
        Err(GraphicsError::InvalidMemory)
    );
    call(
        &mut state,
        &mut memory,
        Function::DropdownGetSelectedString,
        &[dropdown, 200, 32],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(4));
    assert_eq!(memory.bytes(GuestSlice::new(200, 5)).unwrap(), b"Beta\0");
    call(
        &mut state,
        &mut memory,
        Function::DropdownOpen,
        &[dropdown],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::DropdownIsOpenState,
        &[dropdown],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<u32>::new(1)), Some(1));
    call(
        &mut state,
        &mut memory,
        Function::DropdownClose,
        &[dropdown],
        0,
    )
    .unwrap();

    call(
        &mut state,
        &mut memory,
        Function::RollerCreate,
        &[window],
        1,
    )
    .unwrap();
    let roller = handle(&memory);
    memory
        .bytes_mut(GuestSlice::new(100, 10))
        .unwrap()
        .copy_from_slice(b"Sun\nMoon\0\0");
    call(
        &mut state,
        &mut memory,
        Function::RollerSetOptions,
        &[roller, 100, 1],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RollerSetOptions,
            &[roller, 100, 2],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    memory
        .bytes_mut(GuestSlice::new(100, 10))
        .unwrap()
        .fill(b'Y');
    call(
        &mut state,
        &mut memory,
        Function::RollerSetSelected,
        &[roller, 1, 0],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::RollerSetVisibleRowCount,
        &[roller, 5],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::RollerGetSelectedString,
        &[roller, 220, 32],
        1,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(1)), Some(4));
    assert_eq!(memory.bytes(GuestSlice::new(220, 5)).unwrap(), b"Moon\0");

    drop(state);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
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
        Function::LabelCreate,
        &[container],
        1,
    )
    .unwrap();
    let label = handle(&memory);
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
    for _ in 5..256 {
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
