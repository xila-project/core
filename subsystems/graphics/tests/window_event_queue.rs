//! A window's event queue is bounded: bursts neither allocate nor grow it.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use graphics::{EventKind, WINDOW_EVENT_QUEUE_CAPACITY, Window, lvgl};

fn system_used() -> usize {
    testing::memory_crate::global_manager().get_used()
}

#[test]
fn event_bursts_do_not_allocate_and_keep_clicks() {
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    let mut window = unsafe { Window::new(screen).unwrap() };
    let window = unsafe { window.as_mut() };
    let child = unsafe { lvgl::lv_obj_create(window.as_object_mutable()) };
    let send = |code| unsafe {
        lvgl::lv_obj_send_event(child, code, core::ptr::null_mut());
    };
    while window.pop_event().is_some() {}

    // Pointer moves coalesce into one event, whatever the burst size.
    let before = system_used();
    for _ in 0..10_000 {
        send(lvgl::lv_event_code_t_LV_EVENT_PRESSING);
    }
    assert_eq!(system_used(), before, "pointer moves must not allocate");
    assert_eq!(window.pop_event().unwrap().code, EventKind::Pressing);
    assert!(window.pop_event().is_none());
    assert_eq!(window.dropped_event_count(), 0);

    // Distinct events fill the ring and are then dropped and counted: the queue
    // never grows and the first events stay in order.
    for _ in 0..10_000 {
        send(lvgl::lv_event_code_t_LV_EVENT_CLICKED);
    }
    assert_eq!(
        system_used(),
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
        assert_eq!(event.target, child);
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

    window.discard_events_for_target(child);
    window.delete();
}
