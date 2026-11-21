//! A window's event queue is bounded: bursts neither allocate nor grow it.
#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

use graphics::{EventKind, WINDOW_EVENT_QUEUE_CAPACITY, Window, lvgl};
use std::sync::Mutex;

/// LVGL is not thread safe and the tests share its global state.
static LVGL: Mutex<()> = Mutex::new(());

fn thread_allocated_bytes() -> usize {
    testing_support::MemoryManager::thread_allocations().allocated_bytes()
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
/// queue in between. Tree and hover notifications must leave room for input.
#[test]
fn tree_and_hover_notifications_are_never_queued() {
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
    let nested = children[0];
    let window_object = window.as_object_mutable();
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
    unsafe { lvgl::lv_obj_clean(window.as_object_mutable()) };
    assert!(window.pop_event().is_none());
    assert_eq!(window.dropped_event_count(), 0);

    unsafe { lvgl::lv_obj_delete(window.as_object_mutable()) };
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
    let mut window = unsafe { Window::new(screen).unwrap() };
    let window = unsafe { window.as_mut() };
    let page = unsafe { lvgl::lv_obj_create(window.as_object_mutable()) };
    let slider = unsafe { lvgl::lv_obj_create(page) };
    let text = unsafe { lvgl::lv_obj_create(page) };
    let other = unsafe { lvgl::lv_obj_create(window.as_object_mutable()) };
    let send = |target, code| unsafe {
        lvgl::lv_obj_send_event(target, code, core::ptr::null_mut());
    };
    while window.pop_event().is_some() {}

    send(slider, lvgl::lv_event_code_t_LV_EVENT_PRESSED);
    for step in 0..512 {
        send(slider, lvgl::lv_event_code_t_LV_EVENT_PRESSING);
        send(slider, lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED);
        if step == 100 {
            send(slider, lvgl::lv_event_code_t_LV_EVENT_LONG_PRESSED);
        }
        send(slider, lvgl::lv_event_code_t_LV_EVENT_LONG_PRESSED_REPEAT);
        send(text, lvgl::lv_event_code_t_LV_EVENT_SCROLL);
        send(other, lvgl::lv_event_code_t_LV_EVENT_CLICKED);
    }
    send(slider, lvgl::lv_event_code_t_LV_EVENT_RELEASED);
    send(slider, lvgl::lv_event_code_t_LV_EVENT_CLICKED);
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
        send(slider, code);
    }
    while window.pop_event().is_some() {}
    for code in [
        lvgl::lv_event_code_t_LV_EVENT_RELEASED,
        lvgl::lv_event_code_t_LV_EVENT_SHORT_CLICKED,
        lvgl::lv_event_code_t_LV_EVENT_CLICKED,
    ] {
        send(slider, code);
    }
    assert!(
        window.pop_event().is_none(),
        "the end of a rejected press is dropped"
    );

    unsafe { lvgl::lv_obj_delete(window.as_object_mutable()) };
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
    let mut window = unsafe { Window::new(screen).unwrap() };
    let window = unsafe { window.as_mut() };
    let page = unsafe { lvgl::lv_obj_create(window.as_object_mutable()) };
    for _ in 0..8 {
        let child = unsafe { lvgl::lv_obj_create(page) };
        unsafe { lvgl::lv_obj_set_size(child, 20, 10) };
    }
    unsafe { lvgl::lv_obj_set_flex_flow(page, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW_WRAP) };
    while window.pop_event().is_some() {}

    for _ in 0..3 {
        unsafe {
            lvgl::lv_obj_invalidate(window.as_object_mutable());
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

    unsafe { lvgl::lv_obj_delete(window.as_object_mutable()) };
}
