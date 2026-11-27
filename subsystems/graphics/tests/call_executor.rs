//! The graphics call executor, driven without any VM: calls go in by catalog
//! ID, memory comes through a transport, and the process's handles, events and
//! cleanup behave the same whichever transport is in front.
#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

use core::{
    ffi::CStr,
    task::{Context, Poll},
};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::{
    GET_RESOLUTION, Manager, Point, SET_DRAWING_AREA, WAS_RESIZED,
    call::{
        Call, EVENT_CLOSE_REQUESTED, Error, EventCode, Function, LinearMemory, State, Transport,
        Word, execute,
    },
    lvgl,
};
use shared::AnyByLayout;

struct Screen;
impl MountOperations for Screen {}
impl DirectCharacterDevice for Screen {}
impl DirectBaseOperations for Screen {
    fn read(&self, _: &mut Context<'_>, _: &mut [u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
    }
    fn write(
        &self,
        _: &mut Context<'_>,
        bytes: &[u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Ok(bytes.len()))
    }
    fn control(
        &self,
        _: &mut Context<'_>,
        command: ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<file_system::Result<()>> {
        Poll::Ready(match command {
            GET_RESOLUTION::IDENTIFIER => GET_RESOLUTION::cast_output(output)
                .map(|resolution| *resolution = Point::new(320, 240)),
            WAS_RESIZED::IDENTIFIER => WAS_RESIZED::cast_output(output).map(|resized| *resized = 0),
            SET_DRAWING_AREA::IDENTIFIER => Ok(()),
            _ => Err(file_system::IoErrorKind::Unsupported),
        })
    }
}

/// Native-style memory: addresses are host pointers, trusted as they are.
struct HostMemory;
impl Transport for HostMemory {
    fn word_bits(&self) -> u32 {
        usize::BITS
    }
    fn bytes(&self, address: Word, length: usize) -> Result<&[u8], Error> {
        // SAFETY: the test passes addresses of live buffers of this length.
        Ok(unsafe { core::slice::from_raw_parts(address as *const u8, length) })
    }
    fn bytes_mut(&mut self, address: Word, length: usize) -> Result<&mut [u8], Error> {
        // SAFETY: as above, and the buffer is not otherwise borrowed.
        Ok(unsafe { core::slice::from_raw_parts_mut(address as *mut u8, length) })
    }
    fn c_string(&self, address: Word, limit: usize) -> Result<&CStr, Error> {
        if address == 0 {
            return Err(Error::InvalidMemory);
        }
        // A bounded scan, as `strnlen` does: never read past the limit.
        let start = address as *const u8;
        for length in 0..limit {
            // SAFETY: the test passes NUL-terminated strings, and the scan
            // stops at the first NUL.
            if unsafe { *start.add(length) } == 0 {
                // SAFETY: the bytes up to and including this NUL hold no other NUL.
                return Ok(unsafe {
                    CStr::from_bytes_with_nul_unchecked(core::slice::from_raw_parts(
                        start,
                        length + 1,
                    ))
                });
            }
        }
        Err(Error::ResourceLimit)
    }
}

const RESULT: Word = 8;

fn call(
    state: &mut State,
    transport: &mut dyn Transport,
    function: Function,
    arguments: &[Word],
    result: Word,
) -> Result<(), Error> {
    let mut words = [0; 7];
    words[..arguments.len()].copy_from_slice(arguments);
    execute(
        state,
        transport,
        &Call {
            function: function.id(),
            arguments: words,
            count: arguments.len() as i32,
            result,
        },
    )
}

/// Run a call on a 32-bit linear memory and return its word result.
fn word(
    state: &mut State,
    memory: &mut LinearMemory<'_>,
    function: Function,
    args: &[Word],
) -> Word {
    call(state, memory, function, args, RESULT).unwrap();
    let bytes = memory.bytes(RESULT, 4).unwrap();
    u32::from_le_bytes(bytes.try_into().unwrap()) as Word
}

fn i32_result(memory: &LinearMemory<'_>) -> i32 {
    i32::from_le_bytes(memory.bytes(RESULT, 4).unwrap().try_into().unwrap())
}

fn u32_at(memory: &LinearMemory<'_>, address: Word) -> u32 {
    u32::from_le_bytes(memory.bytes(address, 4).unwrap().try_into().unwrap())
}

fn child_count(manager: &Manager, object: *mut lvgl::lv_obj_t) -> u32 {
    manager
        .lock_function_sync(|| Ok(unsafe { lvgl::lv_obj_get_child_count(object) }))
        .unwrap()
}

#[test]
fn executor_runs_calls_for_independent_processes() {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let manager: &'static Manager = Box::leak(Box::new(task::block_on(
        graphics::initialize_with_input_devices(time, &Screen, &[], 1024, false),
    )));
    graphics::set_ffi_manager(manager);

    call_table_keeps_catalog_ids();
    dispatch_rejects_malformed_calls(manager);
    handles_belong_to_one_process(manager);
    generated_adapters_and_kinds(manager);
    every_declared_call_reaches_a_handler(manager);
    pointer_arguments_follow_the_transport(manager);
    events_come_from_the_process_window(manager);
    a_process_that_exits_leaves_nothing(manager);
    system_keyboard_enable_is_idempotent(manager);
}

fn system_keyboard_enable_is_idempotent(manager: &Manager) {
    let screen = manager
        .with_lvgl_sync(|access| manager.get_current_screen(access))
        .unwrap();
    let observer_count = || {
        manager
            .lock_function_sync(|| {
                Ok(unsafe { lvgl::lv_display_get_event_count(lvgl::lv_obj_get_display(screen)) })
            })
            .unwrap()
    };
    let before = observer_count();
    task::block_on(manager.enable_on_screen_keyboard());
    assert_eq!(observer_count(), before + 1);
    task::block_on(manager.enable_on_screen_keyboard());
    assert_eq!(observer_count(), before + 1);
}

fn call_table_keeps_catalog_ids() {
    // Legacy IDs follow the alphabetical catalog; extensions append from 0xf000.
    assert_eq!(Function::ButtonCreate.id(), 28);
    assert_eq!(Function::LabelCreate.id(), 160);
    assert_eq!(Function::ObjectCreate.id(), 248);
    assert_eq!(Function::FilePickerCreate.id(), 0xf02d);
    assert_eq!(Function::WindowPopPointerEvent.id(), 0xf002);
    assert_eq!(Function::BarCreate.id(), 0xf00f);
    for function in [
        Function::WindowCreate,
        Function::ObjectSetPos,
        Function::DropdownGetSelectedString,
        Function::ChartSetValues,
    ] {
        assert_eq!(Function::from_id(function.id()), Some(function));
    }
    assert_eq!(Function::from_id(-1), None);
    assert_eq!(Function::from_id(0xf0ff), None);
    assert_eq!(graphics::call::MAX_ARGUMENTS, 7);
    // The contract's close request lives beside LVGL's own events.
    assert_eq!(EVENT_CLOSE_REQUESTED, EventCode::Last as u32 + 3);
}

/// A call declared without an arm in the handwritten match would answer
/// `Unsupported` at runtime. Every declared ID runs here with zeroed words:
/// a bad handle or value is fine, `Unsupported` is not.
fn every_declared_call_reaches_a_handler(manager: &'static Manager) {
    let mut bytes = [0; 64];
    let mut memory = LinearMemory::new(&mut bytes, 32);
    let mut state = State::new(Some(manager));
    let mut declared = 0;
    for id in 0..=u16::MAX {
        let Some(function) = Function::from_id(i32::from(id)) else {
            continue;
        };
        declared += 1;
        // The argument count is the first one the call table accepts.
        let result = (0..=graphics::call::MAX_ARGUMENTS)
            .map(|count| call(&mut state, &mut memory, function, &[0; 7][..count], RESULT))
            .find(|result| *result != Err(Error::InvalidArgumentsCount))
            .expect("every call has an argument count");
        assert_ne!(
            result,
            Err(Error::Unsupported),
            "{function:?} has no handler"
        );
    }
    assert!(declared > 100, "only {declared} calls are declared");
}

fn dispatch_rejects_malformed_calls(manager: &'static Manager) {
    let mut bytes = [0; 64];
    let mut memory = LinearMemory::new(&mut bytes, 32);
    let mut state = State::new(Some(manager));
    let unknown = Call {
        function: 0x7fff,
        arguments: [0; 7],
        count: 0,
        result: RESULT,
    };
    assert_eq!(
        execute(&mut state, &mut memory, &unknown),
        Err(Error::Unsupported)
    );
    assert_eq!(
        call(&mut state, &mut memory, Function::LabelCreate, &[], RESULT),
        Err(Error::InvalidArgumentsCount)
    );
    // A result that cannot be written stops the call before it has an effect.
    assert_eq!(
        call(&mut state, &mut memory, Function::WindowCreate, &[], 62),
        Err(Error::InvalidMemory)
    );
    assert_eq!(state.handle_count(), 0);
    assert_eq!(
        call(&mut state, &mut memory, Function::WindowCreate, &[], 0),
        Err(Error::InvalidMemory)
    );
    // Without a manager there is nothing to run on.
    let mut detached = State::new(None);
    assert_eq!(
        call(
            &mut detached,
            &mut memory,
            Function::WindowCreate,
            &[],
            RESULT
        ),
        Err(Error::Unavailable)
    );
}

fn handles_belong_to_one_process(manager: &'static Manager) {
    let mut bytes = [0; 64];
    let mut memory = LinearMemory::new(&mut bytes, 32);
    let mut first = State::new(Some(manager));
    let mut second = State::new(Some(manager));
    let window = word(&mut first, &mut memory, Function::WindowCreate, &[]);
    let label = word(&mut first, &mut memory, Function::LabelCreate, &[window]);
    assert_eq!(first.handle_count(), 2);
    assert_eq!(second.handle_count(), 0);
    // Another process cannot name them, and a stale or invented handle fails
    // the same way.
    for handle in [window, label, 0, 0xdead_beef] {
        assert_eq!(
            call(
                &mut second,
                &mut memory,
                Function::ObjectGetX,
                &[handle],
                RESULT
            ),
            Err(Error::InvalidHandle)
        );
    }
    // A handle names one kind of object: a label is not a window.
    assert_eq!(
        call(
            &mut first,
            &mut memory,
            Function::WindowSetIcon,
            &[label, 0, 0],
            RESULT
        ),
        Err(Error::InvalidHandle)
    );
    assert_eq!(
        call(
            &mut first,
            &mut memory,
            Function::ChartSetType,
            &[label, 0],
            RESULT
        ),
        Err(Error::InvalidHandle)
    );
    call(
        &mut first,
        &mut memory,
        Function::ObjectDelete,
        &[label],
        RESULT,
    )
    .unwrap();
    assert_eq!(first.handle_count(), 1);
    assert_eq!(
        call(
            &mut first,
            &mut memory,
            Function::ObjectGetX,
            &[label],
            RESULT
        ),
        Err(Error::InvalidHandle)
    );
    drop(first);
    drop(second);
    assert_eq!(
        manager
            .with_lvgl_sync(|access| manager.get_window_count(access))
            .unwrap(),
        0
    );
}

fn generated_adapters_and_kinds(manager: &'static Manager) {
    let mut bytes = [0; 64];
    let mut memory = LinearMemory::new(&mut bytes, 32);
    let mut state = State::new(Some(manager));
    let window = word(&mut state, &mut memory, Function::WindowCreate, &[]);
    let object = word(&mut state, &mut memory, Function::ObjectCreate, &[window]);
    // Scalar adapters narrow signed words and read back through the result.
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetPos,
        &[object, (-17i32) as u32 as Word, 23],
        RESULT,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectUpdateLayout,
        &[object],
        RESULT,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::ObjectGetX,
        &[object],
        RESULT,
    )
    .unwrap();
    assert_eq!(i32_result(&memory), -17);
    // A 64-bit caller sign-extends an i32 in its word.
    let mut wide = [0; 64];
    let mut wide_memory = LinearMemory::new(&mut wide, 64);
    assert_eq!(
        call(
            &mut state,
            &mut wide_memory,
            Function::ObjectSetPos,
            &[object, 1 << 40, 0],
            RESULT
        ),
        Err(Error::InvalidValue)
    );
    // Beyond 32 bits nothing is an i32, however it is extended.
    assert_eq!(
        call(
            &mut state,
            &mut wide_memory,
            Function::ObjectSetPos,
            &[object, 0x1_ffff_ffff, 0],
            RESULT
        ),
        Err(Error::InvalidValue)
    );
    call(
        &mut state,
        &mut wide_memory,
        Function::ObjectSetPos,
        &[object, (-5isize) as Word, 0],
        RESULT,
    )
    .unwrap();
    // Handles are written as full words for a 64-bit caller.
    call(
        &mut state,
        &mut wide_memory,
        Function::ObjectCreate,
        &[window],
        RESULT,
    )
    .unwrap();
    let handle = u64::from_le_bytes(wide_memory.bytes(RESULT, 8).unwrap().try_into().unwrap());
    assert_ne!(handle, 0);
    assert!(handle <= u64::from(u32::MAX));

    // Generated constructors register the kind that later calls check.
    let button = word(&mut state, &mut memory, Function::ButtonCreate, &[window]);
    let label = word(&mut state, &mut memory, Function::LabelCreate, &[window]);
    call(
        &mut state,
        &mut memory,
        Function::ObjectSetThemeVariant,
        &[button, 1],
        RESULT,
    )
    .unwrap();
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetThemeVariant,
            &[label, 1],
            RESULT
        ),
        Err(Error::InvalidHandle)
    );
    // Extension widgets share one range interface.
    let bar = word(&mut state, &mut memory, Function::BarCreate, &[window]);
    call(
        &mut state,
        &mut memory,
        Function::RangeSetRange,
        &[bar, 0, 200],
        RESULT,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::RangeSetValue,
        &[bar, 150, 0],
        RESULT,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::RangeGetValue,
        &[bar],
        RESULT,
    )
    .unwrap();
    assert_eq!(i32_result(&memory), 150);
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RangeSetValue,
            &[bar, 201, 0],
            RESULT
        ),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::RangeSetRange,
            &[label, 0, 9],
            RESULT
        ),
        Err(Error::InvalidHandle)
    );
    // Each word width accepts only its own form of a negative i32: a 64-bit
    // caller's zero-extended 0xffff_ffff is not -1.
    assert_eq!(
        call(
            &mut state,
            &mut wide_memory,
            Function::ObjectSetPos,
            &[object, 0xffff_ffff, 0],
            RESULT
        ),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::ObjectSetPos,
            &[object, (-1isize) as Word, 0],
            RESULT
        ),
        Err(Error::InvalidValue)
    );
    // Generated validation: the object first, then the value, then the kind.
    let mut check = |function: Function, arguments: &[Word]| {
        call(&mut state, &mut memory, function, arguments, RESULT)
    };
    assert_eq!(check(Function::RangeSetMode, &[bar, 2]), Ok(()));
    assert_eq!(
        check(Function::RangeSetMode, &[bar, 3]),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        check(Function::RangeSetMode, &[label, 3]),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        check(Function::RangeSetMode, &[label, 1]),
        Err(Error::InvalidHandle)
    );
    assert_eq!(
        check(Function::ObjectSetRadius, &[object, 1025, 0]),
        Err(Error::InvalidValue)
    );
    assert_eq!(check(Function::ObjectSetRadius, &[object, 8, 0]), Ok(()));
    assert_eq!(
        check(Function::ObjectSetRadius, &[object, 8, 0x7_0000]),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        check(Function::ObjectSetTextColor, &[object, 0x100_0000, 0]),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        check(Function::ObjectSetFont, &[object, 6, 0]),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        check(Function::ObjectSetTextAlign, &[object, 4, 0]),
        Err(Error::InvalidValue)
    );
    assert_eq!(
        check(Function::ChartSetType, &[label, 0]),
        Err(Error::InvalidHandle)
    );
    assert_eq!(
        check(Function::TextareaSetMaxLength, &[label, 1]),
        Err(Error::InvalidHandle)
    );
    call(&mut state, &mut memory, Function::GridFr, &[3], RESULT).unwrap();
    assert_eq!(i32_result(&memory), unsafe { lvgl::lv_grid_fr(3) });
    assert_eq!(
        call(&mut state, &mut memory, Function::GridFr, &[100], RESULT),
        Err(Error::InvalidValue)
    );
}

fn pointer_arguments_follow_the_transport(manager: &'static Manager) {
    // The same textarea calls, once through offsets into a buffer and once
    // through host pointers.
    let mut state = State::new(Some(manager));
    let mut bytes = [0; 128];
    bytes[120..].fill(b'a');
    bytes[32..38].copy_from_slice(b"hello\0");
    let mut memory = LinearMemory::new(&mut bytes, 32);
    let window = word(&mut state, &mut memory, Function::WindowCreate, &[]);
    let area = word(&mut state, &mut memory, Function::TextareaCreate, &[window]);
    call(
        &mut state,
        &mut memory,
        Function::TextareaSetText,
        &[area, 32],
        RESULT,
    )
    .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::TextareaGetTextLength,
        &[area],
        RESULT,
    )
    .unwrap();
    assert_eq!(i32_result(&memory), 5);
    call(
        &mut state,
        &mut memory,
        Function::TextareaGetText,
        &[area, 64, 6],
        RESULT,
    )
    .unwrap();
    assert_eq!(memory.bytes(64, 6).unwrap(), b"hello\0");
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TextareaGetText,
            &[area, 64, 5],
            RESULT
        ),
        Err(Error::InvalidValue),
        "the buffer must hold the NUL"
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TextareaGetText,
            &[area, 126, 6],
            RESULT
        ),
        Err(Error::InvalidMemory)
    );
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::TextareaSetText,
            &[area, 120],
            RESULT
        ),
        Err(Error::InvalidMemory),
        "a string must end inside memory"
    );

    let mut host = HostMemory;
    let text = c"hosted";
    let mut output = [0u8; 16];
    let mut result = 0i32;
    // Results go to host memory too: the executor never assumes where they live.
    call(
        &mut state,
        &mut host,
        Function::TextareaSetText,
        &[area, text.as_ptr() as usize as Word],
        (&raw mut result) as usize as Word,
    )
    .unwrap();
    call(
        &mut state,
        &mut host,
        Function::TextareaGetText,
        &[area, output.as_mut_ptr() as usize as Word, 16],
        (&raw mut result) as usize as Word,
    )
    .unwrap();
    assert_eq!(result, 6);
    assert_eq!(&output[..7], b"hosted\0");
}

fn events_come_from_the_process_window(manager: &'static Manager) {
    let mut bytes = [0; 64];
    let mut memory = LinearMemory::new(&mut bytes, 32);
    let mut state = State::new(Some(manager));
    let window = word(&mut state, &mut memory, Function::WindowCreate, &[]);
    let button = word(&mut state, &mut memory, Function::ButtonCreate, &[window]);
    let window_pointer = manager
        .with_lvgl_sync(|access| manager.get_window_identifier(access, 0))
        .unwrap() as *mut lvgl::lv_obj_t;
    let button_pointer = manager
        .lock_function_sync(|| Ok(unsafe { lvgl::lv_obj_get_child(window_pointer, 0) }))
        .unwrap();
    // Drain construction events, then click the button.
    loop {
        call(
            &mut state,
            &mut memory,
            Function::WindowPopEvent,
            &[window, 16, 24],
            RESULT,
        )
        .unwrap();
        if u32_at(&memory, 16) == 0 {
            break;
        }
    }
    manager
        .lock_function_sync(|| {
            unsafe {
                lvgl::lv_obj_send_event(
                    button_pointer,
                    lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                    core::ptr::null_mut(),
                );
            }
            Ok(())
        })
        .unwrap();
    call(
        &mut state,
        &mut memory,
        Function::WindowPopPointerEvent,
        &[window, 16, 24, 32, 36, 40, 44],
        RESULT,
    )
    .unwrap();
    assert_eq!(u32_at(&memory, 16), EventCode::Clicked as u32);
    // The target is the caller's handle for the button, not an address.
    assert_eq!(u32_at(&memory, 24) as Word, button);
    // The queue is drained: no event, no target.
    call(
        &mut state,
        &mut memory,
        Function::WindowPopEvent,
        &[window, 16, 24],
        RESULT,
    )
    .unwrap();
    assert_eq!(u32_at(&memory, 16), 0);
    assert_eq!(u32_at(&memory, 24), 0);
    // Unwritable output pointers fail before an event is consumed.
    assert_eq!(
        call(
            &mut state,
            &mut memory,
            Function::WindowPopEvent,
            &[window, 16, 62],
            RESULT
        ),
        Err(Error::InvalidMemory)
    );
}

fn a_process_that_exits_leaves_nothing(manager: &'static Manager) {
    let screen = manager
        .with_lvgl_sync(|access| manager.get_current_screen(access))
        .unwrap();
    let before = child_count(manager, screen);
    let mut bytes = [0; 64];
    let mut memory = LinearMemory::new(&mut bytes, 32);
    for _ in 0..3 {
        let mut state = State::new(Some(manager));
        let window = word(&mut state, &mut memory, Function::WindowCreate, &[]);
        for _ in 0..4 {
            let row = word(&mut state, &mut memory, Function::ObjectCreate, &[window]);
            word(&mut state, &mut memory, Function::LabelCreate, &[row]);
        }
        assert_eq!(state.handle_count(), 9);
        assert_eq!(State::registered_state_count(manager), 1);
        assert_eq!(child_count(manager, screen), before + 1);
        // The process ends without deleting anything itself.
        drop(state);
        assert_eq!(State::registered_state_count(manager), 0);
        assert_eq!(child_count(manager, screen), before);
    }
    let mut state = State::new(Some(manager));
    let window = word(&mut state, &mut memory, Function::WindowCreate, &[]);
    word(&mut state, &mut memory, Function::LabelCreate, &[window]);
    let mut guard = manager.synchronous_lock();
    guard.with_lvgl(|_| drop(state)).unwrap();
    guard
        .with_lvgl(|access| {
            assert_eq!(manager.get_window_count(access).unwrap(), before as usize);
        })
        .unwrap();
    drop(guard);
    assert_eq!(State::registered_state_count(manager), 0);

    assert_eq!(
        manager
            .with_lvgl_sync(|access| manager.get_window_count(access))
            .unwrap(),
        0
    );
}
