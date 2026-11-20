#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

use file_system::{DirectBaseOperations, DirectCharacterDevice, MountOperations, Size};
use graphics::{Input, InputData, InputKind, Key, Point, State, lvgl};
use std::{
    cell::Cell,
    collections::VecDeque,
    ffi::CStr,
    ptr,
    sync::Mutex,
    task::{Context, Poll},
};

struct QueuedInput(Mutex<VecDeque<InputData>>);

impl DirectBaseOperations for QueuedInput {
    fn read(
        &self,
        _: &mut Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        let mut events = self.0.lock().unwrap();
        let Some(event) = events.pop_front() else {
            return Poll::Ready(Ok(0));
        };
        let data: &mut InputData = buffer.try_into().unwrap();
        *data = event;
        data.set_continue(!events.is_empty());
        Poll::Ready(Ok(buffer.len()))
    }

    fn write(&self, _: &mut Context<'_>, _: &[u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
    }
}

impl MountOperations for QueuedInput {}
impl DirectCharacterDevice for QueuedInput {}

/// LVGL owns global state, so these tests must initialize and use it serially.
static LVGL: Mutex<()> = Mutex::new(());

static KEYPAD: QueuedInput = QueuedInput(Mutex::new(VecDeque::new()));
static POINTER: QueuedInput = QueuedInput(Mutex::new(VecDeque::new()));
static BACKLOG: QueuedInput = QueuedInput(Mutex::new(VecDeque::new()));

#[test]
fn keypad_delivers_the_whole_queue_in_one_read() {
    let events = b"aab".iter().copied().flat_map(|byte| {
        [State::Pressed, State::Released]
            .map(|state| InputData::new(Point::default(), state, Key::Character(byte), false))
    });
    KEYPAD.0.lock().unwrap().extend(events);

    let _lvgl = LVGL.lock().unwrap_or_else(|error| error.into_inner());
    let text = unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(128, 128);
        let screen = lvgl::lv_display_get_screen_active(display);
        let textarea = lvgl::lv_textarea_create(screen);
        let group = lvgl::lv_group_create();
        lvgl::lv_group_add_obj(group, textarea);
        lvgl::lv_group_set_default(group);

        let input = Input::new(&KEYPAD, InputKind::Keypad).unwrap();
        lvgl::lv_indev_read(lvgl::lv_indev_get_next(ptr::null_mut()));
        let text = CStr::from_ptr(lvgl::lv_textarea_get_text(textarea))
            .to_str()
            .unwrap()
            .to_owned();

        drop(input);
        lvgl::lv_group_delete(group);
        lvgl::lv_display_delete(display);
        lvgl::lv_deinit();
        text
    };
    assert_eq!(text, "aab");
    assert!(KEYPAD.0.lock().unwrap().is_empty());
}

#[test]
fn pointer_press_and_release_in_one_read_register_a_click() {
    unsafe extern "C" fn count_click(event: *mut lvgl::lv_event_t) {
        let count = unsafe { &*lvgl::lv_event_get_user_data(event).cast::<Cell<u32>>() };
        count.set(count.get() + 1);
    }

    let events = [State::Pressed, State::Released]
        .map(|state| InputData::new(Point::new(20, 20), state, Key::Character(0), false));
    POINTER.0.lock().unwrap().extend(events);

    let _lvgl = LVGL.lock().unwrap_or_else(|error| error.into_inner());
    let clicks = Cell::new(0_u32);
    unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(128, 128);
        let screen = lvgl::lv_display_get_screen_active(display);
        let button = lvgl::lv_button_create(screen);
        lvgl::lv_obj_set_pos(button, 10, 10);
        lvgl::lv_obj_set_size(button, 40, 40);
        lvgl::lv_obj_add_event_cb(
            button,
            Some(count_click),
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            ptr::from_ref(&clicks).cast_mut().cast(),
        );
        lvgl::lv_obj_update_layout(screen);

        let input = Input::new(&POINTER, InputKind::Pointer).unwrap();
        lvgl::lv_indev_read(lvgl::lv_indev_get_next(ptr::null_mut()));

        drop(input);
        lvgl::lv_display_delete(display);
        lvgl::lv_deinit();
    }
    assert_eq!(clicks.get(), 1);
    assert!(POINTER.0.lock().unwrap().is_empty());
}

#[test]
fn a_driver_with_more_events_resumes_on_the_next_read() {
    BACKLOG.0.lock().unwrap().resize(514, InputData::default());

    let _lvgl = LVGL.lock().unwrap_or_else(|error| error.into_inner());
    let (after_first, after_second) = unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(128, 128);
        let input = Input::new(&BACKLOG, InputKind::Pointer).unwrap();
        let indev = lvgl::lv_indev_get_next(ptr::null_mut());

        lvgl::lv_indev_read(indev);
        let after_first = BACKLOG.0.lock().unwrap().len();
        lvgl::lv_indev_read(indev);
        let after_second = BACKLOG.0.lock().unwrap().len();

        drop(input);
        lvgl::lv_display_delete(display);
        lvgl::lv_deinit();
        (after_first, after_second)
    };
    assert_eq!((after_first, after_second), (2, 0));
}
