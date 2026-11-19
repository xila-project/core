//! A window's event queue is bounded: bursts neither allocate nor grow it.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use graphics::{EventKind, WINDOW_EVENT_QUEUE_CAPACITY, Window, lvgl};
use std::sync::Mutex;

/// LVGL is not thread safe and the tests share its global state.
static LVGL: Mutex<()> = Mutex::new(());

fn thread_allocated_bytes() -> usize {
    testing::MemoryManager::thread_allocations().allocated_bytes()
}

#[test]
fn event_bursts_do_not_allocate_and_keep_clicks() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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

    // Distinct events fill the ring and are then dropped and counted: the queue
    // never grows and the first events stay in order.
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

    window.discard_events_for_target(child);
    unsafe { lvgl::lv_obj_delete(window.as_object_mutable()) };
}

/// A double tap queues two clicks on a button; handling the first one rebuilds
/// the list, and LVGL may give a new button the freed button's address.
#[test]
fn queued_events_never_match_widgets_created_after_a_rebuild() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    let mut window = unsafe { Window::new(screen).unwrap() };
    let window = unsafe { window.as_mut() };
    let list = unsafe { lvgl::lv_obj_create(window.as_object_mutable()) };
    let click = |target| unsafe {
        lvgl::lv_obj_send_event(
            target,
            lvgl::lv_event_code_t_LV_EVENT_CLICKED,
            core::ptr::null_mut(),
        );
    };
    let clicks = |window: &mut Window| {
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

    unsafe { lvgl::lv_obj_delete(window.as_object_mutable()) };
}

/// Building a page creates a widget per label and button without draining the
/// queue in between (a guest builds in fuel slices): creating and deleting
/// widgets must not take ring slots, or the next click is dropped.
#[test]
fn building_a_page_does_not_drop_the_next_click() {
    let _lvgl = LVGL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    unsafe { lvgl::lv_init() };
    unsafe { lvgl::lv_display_create(320, 240) };
    let screen = unsafe { lvgl::lv_obj_create(core::ptr::null_mut()) };
    let mut window = unsafe { Window::new(screen).unwrap() };
    let window = unsafe { window.as_mut() };
    while window.pop_event().is_some() {}

    let page = unsafe { lvgl::lv_obj_create(window.as_object_mutable()) };
    // Both directly under the window and nested deeper.
    let mut children: Vec<_> = (0..40)
        .map(|_| unsafe { lvgl::lv_obj_create(page) })
        .collect();
    children.extend((0..40).map(|_| unsafe { lvgl::lv_obj_create(window.as_object_mutable()) }));
    let button = children[79];
    unsafe {
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
    unsafe { lvgl::lv_obj_clean(window.as_object_mutable()) };
    assert!(window.pop_event().is_none());
    assert_eq!(window.dropped_event_count(), 0);

    unsafe { lvgl::lv_obj_delete(window.as_object_mutable()) };
}
