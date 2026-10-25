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
async fn led_handles_expose_bounded_brightness_and_toggle_operations() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 64];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(&mut state, &mut memory, Function::LedCreate, &[window], 1).unwrap();
    let led = handle(&memory);

    call(
        &mut state,
        &mut memory,
        Function::LedSetBrightness,
        &[led, 150],
        0,
    )
    .unwrap();
    let native_window = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        let native_led = unsafe { lvgl::lv_obj_get_child(native_window, 0) };
        assert_eq!(unsafe { lvgl::lv_led_get_brightness(native_led) }, 150);
    }

    call(&mut state, &mut memory, Function::LedOff, &[led], 0).unwrap();
    call(&mut state, &mut memory, Function::LedOn, &[led], 0).unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LedSetBrightness,
            &[window, 1],
            0
        ),
        Err(GraphicsError::InvalidHandle)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn calendar_copies_highlights_and_validates_dates() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 128];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::CalendarCreate,
        &[window],
        1,
    )
    .unwrap();
    let calendar = handle(&memory);

    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CalendarSetToday,
            &[calendar, 2024, 2, 29],
            0,
        ),
        Ok(())
    );
    call(
        &mut state,
        &mut memory,
        Function::CalendarSetMonthShown,
        &[calendar, 2024, 2],
        0,
    )
    .unwrap();

    // CalendarDate is encoded as the guest ABI's 4-byte {u16,u8,u8} record.
    for (offset, date) in [
        (32 as WasmUsize, (2024u16, 2u8, 5u8)),
        (36 as WasmUsize, (2024, 2, 29)),
    ] {
        memory
            .write(GuestPointer::<u16>::new(offset), date.0)
            .unwrap();
        memory
            .write(GuestPointer::<u8>::new(offset + 2), date.1)
            .unwrap();
        memory
            .write(GuestPointer::<u8>::new(offset + 3), date.2)
            .unwrap();
    }
    call(
        &mut state,
        &mut memory,
        Function::CalendarSetHighlights,
        &[calendar, 32, 2],
        0,
    )
    .unwrap();
    memory.write(GuestPointer::<u16>::new(32), 2031).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::CalendarGetShownDate,
        &[calendar, 48],
        0,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<u16>::new(48)), Some(2024));
    assert_eq!(memory.read(GuestPointer::<u8>::new(50)), Some(2));
    assert_eq!(memory.read(GuestPointer::<u8>::new(51)), Some(1));
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CalendarSetHighlights,
            &[calendar, 125, 1],
            0,
        ),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CalendarSetHighlights,
            &[calendar, 0, 33],
            0,
        ),
        Err(GraphicsError::ResourceLimit)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CalendarGetShownDate,
            &[calendar, 126],
            0,
        ),
        Err(GraphicsError::InvalidMemory)
    );

    let native_window = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        let native_calendar = unsafe { lvgl::lv_obj_get_child(native_window, 0) };
        assert_eq!(
            unsafe { lvgl::lv_calendar_get_highlighted_dates_num(native_calendar) },
            2
        );
        let highlighted = unsafe { lvgl::lv_calendar_get_highlighted_dates(native_calendar) };
        assert_eq!(unsafe { (*highlighted).year }, 2024);
        assert_eq!(unsafe { (*highlighted).day }, 5);
    }

    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CalendarSetToday,
            &[calendar, 2025, 2, 29],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::CalendarSetToday,
            &[window, 2024, 2, 29],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn spinbox_checks_range_and_digit_format() {
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
        Function::SpinboxCreate,
        &[window],
        1,
    )
    .unwrap();
    let spinbox = handle(&memory);

    call(
        &mut state,
        &mut memory,
        Function::SpinboxSetRange,
        &[spinbox, (-500i32) as WasmUsize, 500],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinboxSetDigitFormat,
        &[spinbox, 6, 2],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinboxSetStep,
        &[spinbox, 10],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinboxSetValue,
        &[spinbox, (-120i32) as WasmUsize],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinboxIncrement,
        &[spinbox],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinboxGetValue,
        &[spinbox],
        8,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(8)), Some(-110));
    call(
        &mut state,
        &mut memory,
        Function::SpinboxDecrement,
        &[spinbox],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::SpinboxGetValue,
        &[spinbox],
        8,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(8)), Some(-120));

    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::SpinboxSetRange,
            &[spinbox, 20, 10],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::SpinboxSetDigitFormat,
            &[spinbox, 11, 2],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::SpinboxSetValue,
            &[window, 1],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn chart_uses_host_series_tokens_and_bounds_values() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 1024];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(&mut state, &mut memory, Function::ChartCreate, &[window], 1).unwrap();
    let chart = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ChartSetType,
        &[chart, lvgl::lv_chart_type_t_LV_CHART_TYPE_LINE as WasmUsize],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ChartSetPointCount,
        &[chart, 3],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ChartSetAxisRange,
        &[
            chart,
            lvgl::lv_chart_axis_t_LV_CHART_AXIS_PRIMARY_Y as WasmUsize,
            (-100i32) as WasmUsize,
            100,
        ],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ChartAddSeriesSlot,
        &[chart, 0x123456],
        4,
    )
    .unwrap();
    let series = memory.read(GuestPointer::<u32>::new(4)).unwrap();
    assert_eq!(series, 0);

    for (index, value) in [-7i32, 15].into_iter().enumerate() {
        memory
            .write(GuestPointer::<i32>::new(32 + index as WasmUsize * 4), value)
            .unwrap();
    }
    call(
        &mut state,
        &mut memory,
        Function::ChartSetValues,
        &[chart, series as WasmUsize, 32, 2],
        0,
    )
    .unwrap();
    memory.write(GuestPointer::<i32>::new(32), 999).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ChartGetValue,
        &[chart, series as WasmUsize, 0],
        8,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(8)), Some(-7));
    memory.write(GuestPointer::<i32>::new(32), 21).unwrap();
    memory.write(GuestPointer::<i32>::new(36), 31).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ChartSetValues,
        &[chart, series as WasmUsize, 32, 2],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ChartGetValue,
        &[chart, series as WasmUsize, 0],
        8,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(8)), Some(21));
    call(
        &mut state,
        &mut memory,
        Function::ChartGetValue,
        &[chart, series as WasmUsize, 1],
        8,
    )
    .unwrap();
    assert_eq!(memory.read(GuestPointer::<i32>::new(8)), Some(31));
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ChartSetValues,
            &[chart, 8, 32, 1],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ChartSetValues,
            &[chart, series as WasmUsize, 1022, 1],
            0,
        ),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ChartSetValues,
            &[chart, series as WasmUsize, 32, 129],
            0,
        ),
        Err(GraphicsError::ResourceLimit)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn line_copies_bounded_points_and_releases_on_delete() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 8192];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(&mut state, &mut memory, Function::LineCreate, &[window], 1).unwrap();
    let first_line = handle(&memory);

    for (index, value) in [10i32, 20, 30, 40].into_iter().enumerate() {
        memory
            .write(GuestPointer::<i32>::new(64 + index as WasmUsize * 4), value)
            .unwrap();
    }
    call(
        &mut state,
        &mut memory,
        Function::LineSetPoints,
        &[first_line, 64, 2],
        0,
    )
    .unwrap();
    memory.write(GuestPointer::<i32>::new(64), 777).unwrap();
    let native_window = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        let native_line = unsafe { lvgl::lv_obj_get_child(native_window, 0) };
        assert_eq!(unsafe { lvgl::lv_line_get_point_count(native_line) }, 2);
        let native_points = unsafe { lvgl::lv_line_get_points(native_line) };
        assert_eq!(unsafe { (*native_points).x }, 10);
        assert_eq!(unsafe { (*native_points).y }, 20);
    }
    call(
        &mut state,
        &mut memory,
        Function::LineSetYInvert,
        &[first_line, 1],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LineSetYInvert,
            &[window, 1],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LineSetPoints,
            &[first_line, 8190, 1],
            0,
        ),
        Err(GraphicsError::InvalidMemory)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LineSetPoints,
            &[first_line, 64, 129],
            0,
        ),
        Err(GraphicsError::ResourceLimit)
    );

    for index in 0..128u32 {
        memory
            .write(
                GuestPointer::<i32>::new(64 + index as WasmUsize * 8),
                index as i32,
            )
            .unwrap();
        memory
            .write(
                GuestPointer::<i32>::new(68 + index as WasmUsize * 8),
                -(index as i32),
            )
            .unwrap();
    }
    call(
        &mut state,
        &mut memory,
        Function::LineSetPoints,
        &[first_line, 64, 128],
        0,
    )
    .unwrap();
    let mut lines = [first_line; 8];
    for line in lines.iter_mut().skip(1) {
        call(&mut state, &mut memory, Function::LineCreate, &[window], 1).unwrap();
        *line = handle(&memory);
    }
    for line in lines.iter().skip(1) {
        call(
            &mut state,
            &mut memory,
            Function::LineSetPoints,
            &[*line, 64, 128],
            0,
        )
        .unwrap();
    }
    call(&mut state, &mut memory, Function::LineCreate, &[window], 1).unwrap();
    let ninth_line = handle(&memory);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::LineSetPoints,
            &[ninth_line, 64, 128],
            0,
        ),
        Err(GraphicsError::ResourceLimit)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectDelete,
        &[first_line],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::LineSetPoints,
        &[ninth_line, 64, 128],
        0,
    )
    .unwrap();
}

#[xila::task::test(task_path = xila::task)]
async fn list_items_are_bounded_and_return_child_handles() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 256];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(&mut state, &mut memory, Function::ListCreate, &[window], 1).unwrap();
    let list = handle(&memory);
    for (offset, text) in [
        (64, b"Section\0".as_slice()),
        (80, b"Open\0"),
        (96, b"Renamed\0"),
        (112, b"x\0"),
    ] {
        for (index, byte) in text.iter().enumerate() {
            memory
                .write(GuestPointer::<u8>::new(offset + index as WasmUsize), *byte)
                .unwrap();
        }
    }
    call(
        &mut state,
        &mut memory,
        Function::ListAddText,
        &[list, 64],
        1,
    )
    .unwrap();
    let section = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ListAddTextButton,
        &[list, 80],
        1,
    )
    .unwrap();
    let button = handle(&memory);
    let native_window = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    memory.write(GuestPointer::<u8>::new(64), b'!').unwrap();
    memory.write(GuestPointer::<u8>::new(80), b'!').unwrap();
    {
        let _lock = manager.synchronous_lock();
        let native_list = unsafe { lvgl::lv_obj_get_child(native_window, 0) };
        let native_section = unsafe { lvgl::lv_obj_get_child(native_list, 0) };
        let native_button = unsafe { lvgl::lv_obj_get_child(native_list, 1) };
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(lvgl::lv_label_get_text(native_section)) }
                .to_bytes(),
            b"Section"
        );
        assert_eq!(
            unsafe {
                core::ffi::CStr::from_ptr(lvgl::lv_list_get_button_text(native_list, native_button))
            }
            .to_bytes(),
            b"Open"
        );
    }
    call(
        &mut state,
        &mut memory,
        Function::ListSetButtonText,
        &[list, button, 96],
        0,
    )
    .unwrap();
    {
        let _lock = manager.synchronous_lock();
        let native_list = unsafe { lvgl::lv_obj_get_child(native_window, 0) };
        let native_section = unsafe { lvgl::lv_obj_get_child(native_list, 0) };
        let native_button = unsafe { lvgl::lv_obj_get_child(native_list, 1) };
        let section_text = unsafe { lvgl::lv_label_get_text(native_section) };
        let button_text = unsafe { lvgl::lv_list_get_button_text(native_list, native_button) };
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(section_text) }.to_bytes(),
            b"Section"
        );
        assert_eq!(
            unsafe { core::ffi::CStr::from_ptr(button_text) }.to_bytes(),
            b"Renamed"
        );
    }
    assert_ne!(section, button);

    call(&mut state, &mut memory, Function::ListCreate, &[window], 1).unwrap();
    let other_list = handle(&memory);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ListSetButtonText,
            &[other_list, button, 96],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
    memory.write(GuestPointer::<u8>::new(255), b'x').unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ListAddTextButton,
            &[list, 255],
            1,
        ),
        Err(GraphicsError::InvalidMemory)
    );

    for _ in 0..62 {
        call(
            &mut state,
            &mut memory,
            Function::ListAddTextButton,
            &[list, 112],
            1,
        )
        .unwrap();
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ListAddTextButton,
            &[list, 112],
            1,
        ),
        Err(GraphicsError::ResourceLimit)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn tileview_navigation_rejects_foreign_tiles() {
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
        Function::TileviewCreate,
        &[window],
        1,
    )
    .unwrap();
    let view = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::TileviewAddTile,
        &[view, 0, 0, lvgl::lv_dir_t_LV_DIR_ALL as WasmUsize],
        1,
    )
    .unwrap();
    let first = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::TileviewAddTile,
        &[view, 1, 0, lvgl::lv_dir_t_LV_DIR_LEFT as WasmUsize],
        1,
    )
    .unwrap();
    let second = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::TileviewSetTile,
        &[view, second, 0],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::TileviewGetTileActive,
        &[view],
        1,
    )
    .unwrap();
    assert_eq!(handle(&memory), second);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TileviewAddTile,
            &[view, 0, 0, 0],
            1,
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::TileviewSetTile,
        &[view, first, 0],
        0,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::TileviewGetTileActive,
        &[view],
        1,
    )
    .unwrap();
    assert_eq!(handle(&memory), first);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TileviewAddTile,
            &[view, 2, 0, 16],
            1,
        ),
        Err(GraphicsError::InvalidValue)
    );
    let mut last_tile = second;
    for column in 2..32u32 {
        call(
            &mut state,
            &mut memory,
            Function::TileviewAddTile,
            &[view, column as WasmUsize, 0, 0],
            1,
        )
        .unwrap();
        last_tile = handle(&memory);
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TileviewAddTile,
            &[view, 32, 0, 0],
            1,
        ),
        Err(GraphicsError::ResourceLimit)
    );
    call(&mut state, &mut memory, Function::ObjectDelete, &[first], 0).unwrap();
    call(
        &mut state,
        &mut memory,
        Function::TileviewAddTile,
        &[view, 0, 0, 0],
        1,
    )
    .unwrap();
    let _reused_position = handle(&memory);
    assert_ne!(last_tile, _reused_position);

    call(
        &mut state,
        &mut memory,
        Function::TileviewCreate,
        &[window],
        1,
    )
    .unwrap();
    let other_view = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::TileviewAddTile,
        &[other_view, 0, 0, 0],
        1,
    )
    .unwrap();
    let foreign = handle(&memory);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TileviewSetTile,
            &[view, foreign, 0],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn theme_selectors_reject_unknown_ids() {
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
        Function::ButtonCreate,
        &[window],
        1,
    )
    .unwrap();
    let button = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::TextareaCreate,
        &[window],
        1,
    )
    .unwrap();
    let input = handle(&memory);

    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeVariant,
        &[button, 2],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[button, 5],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetSurface,
            &[button, 4],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeError,
        &[input, 1],
        0,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeError,
            &[input, 2],
            0,
        ),
        Err(GraphicsError::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeError,
            &[button, 1],
            0,
        ),
        Err(GraphicsError::InvalidHandle)
    );
}

#[xila::task::test(task_path = xila::task)]
async fn badge_card_and_input_use_shared_theme_roles() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 8192];
    let mut memory = GuestMemory::new(&mut bytes);
    let mut state = GraphicsState::new(Some(manager));
    for (index, byte) in b"Notice\0".iter().enumerate() {
        memory
            .write(GuestPointer::<u8>::new(32 + index as WasmUsize), *byte)
            .unwrap();
    }
    call(&mut state, &mut memory, Function::WindowCreate, &[], 1).unwrap();
    let window = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::BadgeCreate,
        &[window, 32, 4],
        1,
    )
    .unwrap();
    let badge = handle(&memory);
    call(&mut state, &mut memory, Function::CardCreate, &[window], 1).unwrap();
    let card = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::TextareaCreate,
        &[window],
        1,
    )
    .unwrap();
    let input = handle(&memory);
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeError,
        &[input, 1],
        0,
    )
    .unwrap();

    let native_window = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
    {
        let _lock = manager.synchronous_lock();
        let display = unsafe { lvgl::lv_display_get_default() };
        let palette = unsafe { *lvgl::lv_theme_shadcn_get_palette(display) };
        let native_badge = unsafe { lvgl::lv_obj_get_child(native_window, 0) };
        let native_card = unsafe { lvgl::lv_obj_get_child(native_window, 1) };
        let native_input = unsafe { lvgl::lv_obj_get_child(native_window, 2) };
        let badge_background = unsafe { lvgl::lv_obj_get_style_bg_color(native_badge, 0) };
        let card_background = unsafe { lvgl::lv_obj_get_style_bg_color(native_card, 0) };
        let input_border = unsafe { lvgl::lv_obj_get_style_border_color(native_input, 0) };
        assert!(unsafe { lvgl::lv_color_eq(badge_background, palette.destructive) });
        assert!(unsafe { lvgl::lv_color_eq(card_background, palette.card) });
        assert!(unsafe { lvgl::lv_color_eq(input_border, palette.destructive) });
    }
    assert_ne!(badge, card);

    for index in 256..1279 {
        memory
            .write(GuestPointer::<u8>::new(index as WasmUsize), b'x')
            .unwrap();
    }
    memory.write(GuestPointer::<u8>::new(1279), 0).unwrap();
    for _ in 0..7 {
        call(
            &mut state,
            &mut memory,
            Function::BadgeCreate,
            &[window, 256, 0],
            1,
        )
        .unwrap();
    }
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::BadgeCreate,
            &[window, 256, 0],
            1,
        ),
        Err(GraphicsError::ResourceLimit)
    );
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
