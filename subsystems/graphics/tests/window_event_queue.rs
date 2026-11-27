//! A window's event queue is bounded: bursts neither allocate nor grow it.
#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

use graphics::{
    EventKind, InputData, Key, Point, State, WINDOW_EVENT_QUEUE_CAPACITY, Window, lvgl,
};
use std::{
    ptr::{self, NonNull},
    sync::Mutex,
};

/// LVGL is not thread safe and the tests share its global state.
static LVGL: Mutex<()> = Mutex::new(());

/// Access a raw window while the test holds LVGL.
#[derive(Clone, Copy)]
struct TestWindow(NonNull<Window>);

impl TestWindow {
    /// The test holds LVGL and deletes this window only after its last use.
    unsafe fn new(screen: *mut lvgl::lv_obj_t) -> Self {
        // SAFETY: the caller holds LVGL and supplies a live screen.
        Self(unsafe { Window::new(screen).unwrap() })
    }

    fn object(self) -> *mut lvgl::lv_obj_t {
        self.0.as_ptr().cast()
    }

    fn pop_event(self) -> Option<graphics::Event> {
        // SAFETY: the test keeps the window live and holds LVGL.
        unsafe { Window::pop_event(self.0) }
    }

    fn dropped_event_count(self) -> u32 {
        // SAFETY: the test keeps the window live and holds LVGL.
        unsafe { Window::dropped_event_count(self.0) }
    }
}

fn thread_allocated_bytes() -> usize {
    testing_support::MemoryManager::thread_allocations().allocated_bytes()
}

#[test]
fn event_bursts_do_not_allocate_and_keep_clicks() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    // SAFETY: the test holds LVGL and keeps the screen live.
    let window = unsafe { TestWindow::new(screen) };
    let child = unsafe { lvgl::lv_obj_create(window.object()) };
    let send = |code| unsafe {
        lvgl::lv_obj_send_event(child, code, core::ptr::null_mut());
    };
    while window.pop_event().is_some() {}

    // Pointer moves coalesce into one event, whatever the burst size.
    let before = thread_allocated_bytes();
    for _ in 0..10_000 {
        send(lvgl::lv_event_code_t_LV_EVENT_PRESSING);
    }
    assert_eq!(
        thread_allocated_bytes(),
        before,
        "pointer moves must not allocate"
    );
    assert_eq!(window.pop_event().unwrap().code, EventKind::Pressing);
    assert!(window.pop_event().is_none());
    assert_eq!(window.dropped_event_count(), 0);

    // Clicks fill the ring. Clicks are never evicted: further ones are
    // rejected, and each rejection is counted.
    let before = thread_allocated_bytes();
    for _ in 0..10_000 {
        send(lvgl::lv_event_code_t_LV_EVENT_CLICKED);
    }
    assert_eq!(
        thread_allocated_bytes(),
        before,
        "events past the capacity must not allocate"
    );
    assert_eq!(
        window.dropped_event_count() as usize,
        10_000 - WINDOW_EVENT_QUEUE_CAPACITY
    );
    let mut queued = 0;
    while let Some(event) = window.pop_event() {
        assert_eq!(event.code, EventKind::Clicked);
        assert!(event.is_target(child));
        queued += 1;
    }
    assert_eq!(queued, WINDOW_EVENT_QUEUE_CAPACITY);

    // A click restyles every affected object and may scroll: those events must
    // not crowd its release, click or value change out of the ring. LVGL state
    // is global, so this stays in the same test. State changes have no kind and
    // are never queued.
    unsafe { lvgl::lv_obj_add_state(child, lvgl::LV_STATE_PRESSED) };
    unsafe { lvgl::lv_obj_remove_state(child, lvgl::LV_STATE_PRESSED) };
    while let Some(event) = window.pop_event() {
        assert_ne!(event.code, EventKind::Last);
    }

    let input = [
        lvgl::lv_event_code_t_LV_EVENT_PRESSED,
        lvgl::lv_event_code_t_LV_EVENT_RELEASED,
        lvgl::lv_event_code_t_LV_EVENT_CLICKED,
        lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
    ];
    for code in input {
        for _ in 0..WINDOW_EVENT_QUEUE_CAPACITY {
            send(lvgl::lv_event_code_t_LV_EVENT_STYLE_CHANGED);
            send(lvgl::lv_event_code_t_LV_EVENT_SCROLL);
        }
        send(code);
    }

    let queued: Vec<_> = core::iter::from_fn(|| window.pop_event())
        .map(|event| event.code)
        .filter(|code| !matches!(code, EventKind::StyleChanged | EventKind::Scroll))
        .collect();
    assert_eq!(
        queued,
        [
            EventKind::Pressed,
            EventKind::Released,
            EventKind::Clicked,
            EventKind::ValueChanged
        ]
    );

    // SAFETY: the test keeps the window live and holds LVGL.
    unsafe { Window::discard_events_for_target(window.0, child) };
    unsafe { lvgl::lv_obj_delete(window.object()) };
}

/// A double tap queues two clicks on a button; handling the first one rebuilds
/// the list, and LVGL may give a new button the freed button's address.
#[test]
fn queued_events_never_match_widgets_created_after_a_rebuild() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    // SAFETY: the test holds LVGL and keeps the screen live.
    let window = unsafe { TestWindow::new(screen) };
    let list = unsafe { lvgl::lv_obj_create(window.object()) };
    let click = |target| unsafe {
        lvgl::lv_obj_send_event(
            target,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    };
    let clicks = |window: TestWindow| {
        let mut targets = Vec::new();
        while let Some(event) = window.pop_event() {
            if event.code == EventKind::Clicked {
                targets.push(event);
            }
        }
        targets
    };
    while window.pop_event().is_some() {}

    // Rebuild by cleaning the list: the clean deletes the old buttons.
    let old = unsafe { lvgl::lv_obj_create(list) };
    click(old);
    click(old);
    unsafe { lvgl::lv_obj_clean(list) };
    let new = unsafe { lvgl::lv_obj_create(list) };
    let queued = clicks(window);
    assert_eq!(queued.len(), 2, "the events are kept");
    for event in &queued {
        assert!(
            !event.is_target(new),
            "a stale click reached the new button"
        );
        assert!(!event.is_target(old));
    }

    // The same through single deletions, and for a widget moved out of the
    // window's tree.
    click(new);
    click(new);
    unsafe { lvgl::lv_obj_delete(new) };
    let replacement = unsafe { lvgl::lv_obj_create(list) };
    click(replacement);
    let moved_out = unsafe { lvgl::lv_obj_create(list) };
    click(moved_out);
    unsafe { lvgl::lv_obj_set_parent(moved_out, screen) };
    let after = unsafe { lvgl::lv_obj_create(list) };
    let queued = clicks(window);
    assert_eq!(queued.len(), 4);
    let live: Vec<_> = queued.iter().filter(|e| e.is_target(replacement)).collect();
    assert_eq!(live.len(), 1, "only the click on the live button stays");
    assert!(queued.iter().all(|e| !e.is_target(after)));
    assert!(queued.iter().all(|e| !e.is_target(moved_out)));

    unsafe { lvgl::lv_obj_delete(window.object()) };
}

/// Building a page creates a widget per label and button without draining the
/// queue in between. Tree and hover notifications must leave room for input.
#[test]
fn tree_and_hover_notifications_are_never_queued() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    // SAFETY: the test holds LVGL and keeps the screen live.
    let window = unsafe { TestWindow::new(screen) };
    while window.pop_event().is_some() {}

    let page = unsafe { lvgl::lv_obj_create(window.object()) };
    // Both directly under the window and nested deeper.
    let mut children: Vec<_> = (0..40)
        .map(|_| unsafe { lvgl::lv_obj_create(page) })
        .collect();
    children.extend((0..40).map(|_| unsafe { lvgl::lv_obj_create(window.object()) }));
    let button = children[79];
    let nested = children[0];
    let window_object = window.object();
    unsafe {
        // Hover and layout notifications bubble up from the widgets and are
        // filtered by the window.
        for target in [window_object as *mut _, button, nested, page] {
            for code in [
                lvgl::lv_event_code_t_LV_EVENT_HOVER_OVER,
                lvgl::lv_event_code_t_LV_EVENT_HOVER_LEAVE,
                lvgl::lv_event_code_t_LV_EVENT_LAYOUT_CHANGED,
            ] {
                lvgl::lv_obj_send_event(target, code, core::ptr::null_mut());
            }
        }
        lvgl::lv_obj_send_event(
            button,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    }
    assert_eq!(window.dropped_event_count(), 0);
    let queued = core::iter::from_fn(|| window.pop_event()).collect::<Vec<_>>();
    assert_eq!(
        queued.iter().map(|e| e.code).collect::<Vec<_>>(),
        [EventKind::Clicked],
        "only the click is queued"
    );
    assert_eq!(queued[0].code, EventKind::Clicked);
    assert!(queued[0].is_target(button));

    // Deleting the widgets queues nothing either.
    unsafe { lvgl::lv_obj_clean(page) };
    unsafe { lvgl::lv_obj_clean(window.object()) };
    assert!(window.pop_event().is_none());
    assert_eq!(window.dropped_event_count(), 0);

    unsafe { lvgl::lv_obj_delete(window.object()) };
}

/// A drag or a held button inside one LVGL poll: hundreds of moves and value
/// changes, plus unrelated clicks and keys, arrive before the consumer runs.
/// The consumer must still receive the press with its release and clicks.
#[test]
fn a_press_survives_a_burst_bubbled_from_child_widgets() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    // SAFETY: the test holds LVGL and keeps the screen live.
    let window = unsafe { TestWindow::new(screen) };
    let page = unsafe { lvgl::lv_obj_create(window.object()) };
    let slider = unsafe { lvgl::lv_obj_create(page) };
    let text = unsafe { lvgl::lv_obj_create(page) };
    let other = unsafe { lvgl::lv_obj_create(window.object()) };
    let send = |target, code| unsafe {
        lvgl::lv_obj_send_event(target, code, core::ptr::null_mut());
    };
    // LVGL passes the input device with every press event. The pointer is
    // held: the window forgets the press of a released device.
    let held = unsafe {
        HeldDevice::new(
            lvgl::lv_indev_type_t_LV_INDEV_TYPE_POINTER,
            lvgl::lv_display_get_default(),
        )
    };
    unsafe { held.read(Point::new(1, 1), State::Pressed, Key::Character(0)) };
    let pointer = held.device;
    let press = |target, code| unsafe {
        lvgl::lv_obj_send_event(target, code, pointer.cast());
    };
    while window.pop_event().is_some() {}

    press(slider, lvgl::lv_event_code_t_LV_EVENT_PRESSED);
    for step in 0..512 {
        press(slider, lvgl::lv_event_code_t_LV_EVENT_PRESSING);
        press(slider, lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED);
        if step == 100 {
            press(slider, lvgl::lv_event_code_t_LV_EVENT_LONG_PRESSED);
        }
        press(slider, lvgl::lv_event_code_t_LV_EVENT_LONG_PRESSED_REPEAT);
        send(text, lvgl::lv_event_code_t_LV_EVENT_SCROLL);
        send(other, lvgl::lv_event_code_t_LV_EVENT_CLICKED);
    }
    press(slider, lvgl::lv_event_code_t_LV_EVENT_RELEASED);
    press(slider, lvgl::lv_event_code_t_LV_EVENT_CLICKED);
    assert!(window.dropped_event_count() > 0, "the burst overflows");

    let queued: Vec<_> = core::iter::from_fn(|| window.pop_event()).collect();
    let slider_lifecycle: Vec<_> = queued
        .iter()
        .filter(|event| event.is_target(slider))
        .map(|event| event.code)
        .filter(|code| {
            !matches!(
                code,
                EventKind::Pressing | EventKind::ValueChanged | EventKind::LongPressedRepeat
            )
        })
        .collect();
    assert_eq!(
        slider_lifecycle,
        [
            EventKind::Pressed,
            EventKind::LongPressed,
            EventKind::Released,
            EventKind::Clicked
        ]
    );
    assert!(queued.iter().any(|event| event.is_target(other)));
    assert!(queued.len() <= WINDOW_EVENT_QUEUE_CAPACITY);

    // While the queue is full of clicks, a new press is rejected whole.
    for _ in 0..WINDOW_EVENT_QUEUE_CAPACITY {
        send(other, lvgl::lv_event_code_t_LV_EVENT_CLICKED);
    }
    for code in [
        lvgl::lv_event_code_t_LV_EVENT_PRESSED,
        lvgl::lv_event_code_t_LV_EVENT_PRESSING,
    ] {
        press(slider, code);
    }
    while window.pop_event().is_some() {}
    for code in [
        lvgl::lv_event_code_t_LV_EVENT_RELEASED,
        lvgl::lv_event_code_t_LV_EVENT_SHORT_CLICKED,
        lvgl::lv_event_code_t_LV_EVENT_CLICKED,
    ] {
        press(slider, code);
    }
    assert!(
        window.pop_event().is_none(),
        "the end of a rejected press is dropped"
    );

    unsafe { held.delete() };
    unsafe { lvgl::lv_obj_delete(window.object()) };
}

/// Rendering a window and its widgets sends drawing, cover check and layout
/// notifications to every object: none of them may take a slot.
#[test]
fn rendering_notifications_are_never_queued() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    const WIDTH: i32 = 64;
    const HEIGHT: i32 = 48;
    unsafe extern "C" fn flush(
        display: *mut lvgl::lv_display_t,
        _: *const lvgl::lv_area_t,
        _: *mut u8,
    ) {
        unsafe { lvgl::lv_display_flush_ready(display) };
    }
    // LVGL keeps the buffer for the rest of the process.
    let pixels = Box::leak(vec![0_u32; (WIDTH * HEIGHT) as usize].into_boxed_slice());
    let display = unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(WIDTH, HEIGHT);
        lvgl::lv_display_set_color_format(
            display,
            lvgl::lv_color_format_t_LV_COLOR_FORMAT_XRGB8888,
        );
        lvgl::lv_display_set_buffers(
            display,
            pixels.as_mut_ptr().cast(),
            core::ptr::null_mut(),
            size_of_val(pixels) as u32,
            lvgl::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
        );
        lvgl::lv_display_set_flush_cb(display, Some(flush));
        display
    };
    let screen = unsafe { lvgl::lv_display_get_screen_active(display) };
    // SAFETY: the test holds LVGL and keeps the screen live.
    let window = unsafe { TestWindow::new(screen) };
    let page = unsafe { lvgl::lv_obj_create(window.object()) };
    for _ in 0..8 {
        let child = unsafe { lvgl::lv_obj_create(page) };
        unsafe { lvgl::lv_obj_set_size(child, 20, 10) };
    }
    unsafe { lvgl::lv_obj_set_flex_flow(page, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW_WRAP) };
    while window.pop_event().is_some() {}

    for _ in 0..3 {
        unsafe {
            lvgl::lv_obj_invalidate(window.object());
            lvgl::lv_obj_set_width(page, lvgl::lv_obj_get_width(page) - 4);
            lvgl::lv_refr_now(display);
        }
    }
    let queued: Vec<_> = core::iter::from_fn(|| window.pop_event())
        .map(|event| event.code)
        .collect();
    for code in &queued {
        assert!(
            !matches!(
                code,
                EventKind::CoverCheck
                    | EventKind::RefreshExtDrawSize
                    | EventKind::DrawMainBegin
                    | EventKind::DrawMain
                    | EventKind::DrawMainEnd
                    | EventKind::DrawPostBegin
                    | EventKind::DrawPost
                    | EventKind::DrawPostEnd
                    | EventKind::DrawTaskAdded
                    | EventKind::GetSelfSize
                    | EventKind::LayoutChanged
                    | EventKind::HitTest
                    | EventKind::InvalidateArea
                    | EventKind::HoverOver
                    | EventKind::HoverLeave
            ),
            "{code:?} was queued"
        );
    }
    assert_eq!(window.dropped_event_count(), 0);

    unsafe { lvgl::lv_obj_delete(window.object()) };
}

/// The window is resized while a list inside it is flung and the consumer is
/// busy: the bubbled scroll burst must not push the resize out.
#[test]
fn a_scroll_burst_keeps_the_window_resize() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    // SAFETY: the test holds LVGL and keeps the screen live.
    let window = unsafe { TestWindow::new(screen) };
    let list = unsafe { lvgl::lv_obj_create(window.object()) };
    unsafe { lvgl::lv_obj_update_layout(window.object()) };
    while window.pop_event().is_some() {}

    unsafe {
        lvgl::lv_obj_set_size(window.object(), 200, 120);
        lvgl::lv_obj_update_layout(window.object());
    }
    for step in 0..500 {
        let code = match step % 50 {
            0 => lvgl::lv_event_code_t_LV_EVENT_SCROLL_BEGIN,
            49 => lvgl::lv_event_code_t_LV_EVENT_SCROLL_END,
            _ => lvgl::lv_event_code_t_LV_EVENT_SCROLL,
        };
        unsafe { lvgl::lv_obj_send_event(list, code, core::ptr::null_mut()) };
    }
    assert!(window.dropped_event_count() > 0, "the burst overflows");

    let window_object = window.object();
    let resizes = core::iter::from_fn(|| window.pop_event())
        .filter(|event| event.code == EventKind::SizeChanged && event.is_target(window_object))
        .count();
    assert_eq!(resizes, 1, "the latest resize reaches the consumer");

    unsafe { lvgl::lv_obj_delete(window.object()) };
}

/// An LVGL input device that reports `held` on every read.
struct HeldDevice {
    device: *mut lvgl::lv_indev_t,
    held: Box<std::cell::Cell<InputData>>,
}

impl HeldDevice {
    unsafe fn new(kind: lvgl::lv_indev_type_t, display: *mut lvgl::lv_display_t) -> Self {
        unsafe extern "C" fn read(device: *mut lvgl::lv_indev_t, data: *mut lvgl::lv_indev_data_t) {
            unsafe {
                let held =
                    &*lvgl::lv_indev_get_user_data(device).cast::<std::cell::Cell<InputData>>();
                *data = held.get().into();
            }
        }
        let held = Box::new(std::cell::Cell::new(InputData::default()));
        let device = unsafe {
            let device = lvgl::lv_indev_create();
            lvgl::lv_indev_set_type(device, kind);
            lvgl::lv_indev_set_display(device, display);
            lvgl::lv_indev_set_read_cb(device, Some(read));
            lvgl::lv_indev_set_user_data(device, ptr::from_ref(&*held).cast_mut().cast());
            device
        };
        Self { device, held }
    }

    /// Report `state` at `point` with `key`, and let LVGL read it now.
    unsafe fn read(&self, point: Point, state: State, key: Key) {
        self.held.set(InputData::new(point, state, key, false));
        unsafe { lvgl::lv_indev_read(self.device) };
    }

    unsafe fn delete(self) {
        unsafe { lvgl::lv_indev_delete(self.device) };
    }
}

/// A window on the active screen with one widget at (10, 10), 40 by 40.
unsafe fn window_with_widget(
    display: *mut lvgl::lv_display_t,
) -> (TestWindow, *mut lvgl::lv_obj_t) {
    unsafe {
        let screen = lvgl::lv_display_get_screen_active(display);
        // SAFETY: the caller holds LVGL and the display keeps the screen live.
        let window = TestWindow::new(screen);
        let widget = lvgl::lv_obj_create(window.object());
        lvgl::lv_obj_set_pos(widget, 10, 10);
        lvgl::lv_obj_set_size(widget, 40, 40);
        lvgl::lv_obj_update_layout(screen);
        while window.pop_event().is_some() {}
        (window, widget)
    }
}

/// Send `count` clicks by code to `target`, and return how many the window
/// rejected.
unsafe fn rejected_clicks(window: TestWindow, target: *mut lvgl::lv_obj_t, count: usize) -> u32 {
    let before = window.dropped_event_count();
    for _ in 0..count {
        unsafe {
            lvgl::lv_obj_send_event(
                target,
                lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                ptr::null_mut(),
            )
        };
    }
    window.dropped_event_count() - before
}

/// LVGL never ends a press whose input device is deleted, or reset, while it
/// is held: the window gives its reservation back.
#[test]
fn a_press_whose_device_is_deleted_or_reset_gives_its_reservation_back() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(320, 240);
        let (window, widget) = window_with_widget(display);

        for reset in [false, true] {
            let pointer = HeldDevice::new(lvgl::lv_indev_type_t_LV_INDEV_TYPE_POINTER, display);
            pointer.read(Point::new(20, 20), State::Pressed, Key::Character(0));
            let queued: Vec<_> = core::iter::from_fn(|| window.pop_event())
                .map(|event| event.code)
                .collect();
            assert_eq!(queued.first(), Some(&EventKind::Pressed), "{queued:?}");
            assert_eq!(
                rejected_clicks(window, widget, WINDOW_EVENT_QUEUE_CAPACITY),
                4,
                "the held press keeps four slots"
            );
            while window.pop_event().is_some() {}

            if reset {
                // LVGL forgets the pressed widget, and never ends the press.
                lvgl::lv_indev_reset(pointer.device, ptr::null_mut());
                let queued: Vec<_> = core::iter::from_fn(|| window.pop_event())
                    .map(|event| event.code)
                    .collect();
                assert!(queued.contains(&EventKind::InputDeviceReset), "{queued:?}");
                assert_eq!(
                    rejected_clicks(window, widget, WINDOW_EVENT_QUEUE_CAPACITY),
                    0
                );
                while window.pop_event().is_some() {}
                pointer.read(Point::new(20, 20), State::Released, Key::Character(0));
                pointer.delete();
            } else {
                pointer.delete();
                while window.pop_event().is_some() {}
                assert_eq!(
                    rejected_clicks(window, widget, WINDOW_EVENT_QUEUE_CAPACITY),
                    0
                );
            }
            while window.pop_event().is_some() {}
        }

        lvgl::lv_obj_delete(window.object());
        lvgl::lv_display_delete(display);
    }
}

/// A keypad ends its press on the widget focused at release. When the focus
/// moves to another window during the press, this window never gets the end:
/// it gives the reservation back once the keypad is released.
#[test]
fn a_keypad_press_ended_in_another_window_gives_its_reservation_back() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(320, 240);
        let (window, inside) = window_with_widget(display);
        let (other_window, outside) = window_with_widget(display);
        let group = lvgl::lv_group_create();
        lvgl::lv_group_add_obj(group, inside);
        lvgl::lv_group_add_obj(group, outside);
        lvgl::lv_group_focus_obj(inside);
        let keypad = HeldDevice::new(lvgl::lv_indev_type_t_LV_INDEV_TYPE_KEYPAD, display);
        lvgl::lv_indev_set_group(keypad.device, group);
        while window.pop_event().is_some() {}
        while other_window.pop_event().is_some() {}

        keypad.read(Point::default(), State::Pressed, Key::Enter);
        lvgl::lv_group_focus_obj(outside);
        keypad.read(Point::default(), State::Released, Key::Enter);

        let codes = |window: TestWindow| -> Vec<_> {
            core::iter::from_fn(|| window.pop_event())
                .filter(|event| {
                    !matches!(
                        event.code,
                        EventKind::Focused
                            | EventKind::Defocused
                            | EventKind::Key
                            | EventKind::StyleChanged
                    )
                })
                .map(|event| event.code)
                .collect()
        };
        assert_eq!(codes(window), [EventKind::Pressed]);
        let ended = codes(other_window);
        assert_eq!(ended.first(), Some(&EventKind::Released), "{ended:?}");
        assert_eq!(ended.last(), Some(&EventKind::Clicked), "{ended:?}");
        assert_eq!(
            rejected_clicks(window, inside, WINDOW_EVENT_QUEUE_CAPACITY),
            0,
            "the released keypad holds no reservation"
        );

        keypad.delete();
        lvgl::lv_group_delete(group);
        lvgl::lv_obj_delete(other_window.object());
        lvgl::lv_obj_delete(window.object());
        lvgl::lv_display_delete(display);
    }
}
