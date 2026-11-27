//! A window belongs to its `OwnedWindow`: safe code can edit its content but
//! never delete it, and queued event targets are never dereferenced after
//! safe code deleted them (xila-project/core#661).
#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::lvgl_rust::{self, Obj, ScreenAnim, ScreenLoadAnim, WidgetExt};
use graphics::{EventKind, GET_RESOLUTION, Manager, Point, SET_DRAWING_AREA, WAS_RESIZED, lvgl};
use portable_atomic::{AtomicU32, Ordering};
use shared::AnyByLayout;

static TEST_TICKS: AtomicU32 = AtomicU32::new(0);

extern "C" fn test_ticks() -> u32 {
    TEST_TICKS.load(Ordering::Relaxed)
}

struct Screen;
impl MountOperations for Screen {}
impl DirectCharacterDevice for Screen {}
impl DirectBaseOperations for Screen {
    fn read(&self, _: &mut Context<'_>, _: &mut [u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Ok(0))
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

fn child_count(manager: &Manager, object: *mut lvgl::lv_obj_t) -> u32 {
    manager
        .lock_function_sync(|| Ok(unsafe { lvgl::lv_obj_get_child_count(object) }))
        .unwrap()
}

/// Tries every structural operation of the safe layer on the screen's first
/// child, and on the screen itself.
fn safe_edits_of_first_child(manager: &Manager) -> [Result<(), lvgl_rust::Error>; 5] {
    manager
        .with_lvgl_sync(|access| {
            // The issue's reproduction: no `unsafe`.
            let delete = access
                .edit(|access| access.screen_active()?.child(0))
                .unwrap()
                .delete();
            let delete_later = access
                .screen_active()
                .unwrap()
                .child(0)
                .unwrap()
                .delete_later();
            let move_to_index = access
                .edit(|access| access.screen_active()?.child(0))
                .unwrap()
                .move_to_index(-1);
            let set_parent = access
                .edit(|access| access.screen_active()?.child(0))
                .unwrap()
                .set_parent_to(|access| access.screen_active());
            let clean_screen = access.screen_active_mut().unwrap().clean();
            Ok([
                delete,
                delete_later,
                move_to_index,
                set_parent,
                clean_screen,
            ])
        })
        .unwrap()
}

#[test]
fn safe_code_cannot_delete_an_owned_window() {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let manager: &'static Manager = Box::leak(Box::new(task::block_on(
        graphics::initialize_with_input_devices(time, &Screen, &[], 1024, false),
    )));
    // Input registration waits for the same guard as the LVGL loop.
    let guard = manager.synchronous_lock();
    let mut register_input = pin!(manager.add_input_device(&Screen, graphics::InputKind::Keypad));
    let mut context = Context::from_waker(Waker::noop());
    assert!(register_input.as_mut().poll(&mut context).is_pending());
    drop(guard);
    task::block_on(register_input).unwrap();

    // The default parent follows the active screen after a safe screen load.
    let _active_screen = manager
        .with_lvgl_sync(|access| {
            access.set_tick_cb(test_ticks);
            let old = access.screen_active().unwrap().handle()?;
            let screen = access.create_screen()?;
            access.load_screen(
                screen.handle(),
                ScreenAnim::new(ScreenLoadAnim::FADE_IN, 500, 0, true),
            )?;
            assert_eq!(
                access.screen_active().unwrap().as_ptr(),
                old.get(access)?.as_ptr()
            );
            assert!(matches!(
                manager.create_window(access),
                Err(graphics::Error::AlreadyInUse)
            ));

            // Start the transition, then advance past its duration.
            for _ in 0..10 {
                TEST_TICKS.fetch_add(100, Ordering::Relaxed);
                access.timer_handler();
            }
            assert!(old.get(access).is_err());
            Ok(screen)
        })
        .unwrap();

    let screen = manager
        .with_lvgl_sync(|access| manager.get_current_screen(access))
        .unwrap();

    // A top-level window: its only ancestor is the screen, a plain object.
    let mut window = manager
        .with_lvgl_sync(|access| manager.create_window(access))
        .unwrap();
    let window_object = window.as_object();
    assert_eq!(child_count(manager, screen), 1);
    for (operation, result) in ["delete", "delete later", "move", "reparent", "clean"]
        .into_iter()
        .zip(safe_edits_of_first_child(manager))
    {
        assert!(result.is_err(), "safe {operation} must spare the window");
    }
    // Deferred requests run in the timer handler: none may delete it.
    manager
        .with_lvgl_sync(|access| {
            access.timer_handler();
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, screen), 1);
    // A screen load cannot delete the screen holding the window either.
    let loaded = manager
        .with_lvgl_sync(|access| {
            let other = access.create_screen()?;
            let auto_delete = ScreenAnim::new(ScreenLoadAnim::NONE, 0, 0, true);
            Ok(access.load_screen(other.handle(), auto_delete))
        })
        .unwrap();
    assert!(
        loaded.is_err(),
        "auto-deleting the screen must spare the window"
    );

    // Safe code may build and delete the window's content.
    let child = manager
        .with_lvgl_sync(|access| {
            let mut window = access
                .edit(|access| access.screen_active()?.child(0))
                .unwrap();
            assert_eq!(window.view().as_ptr(), window_object);
            Ok(Obj::create(&mut window)?.as_ptr())
        })
        .unwrap();
    assert_eq!(child_count(manager, window_object), 1);

    // An event for the child is queued, then safe code deletes the child.
    manager
        .with_lvgl_sync(|access| {
            while window.pop_event(access).is_some() {}
            Ok(())
        })
        .unwrap();
    manager
        .lock_function_sync(|| {
            unsafe {
                lvgl::lv_obj_send_event(
                    child,
                    lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                    core::ptr::null_mut(),
                );
            }
            Ok(())
        })
        .unwrap();
    let event = manager
        .with_lvgl_sync(|access| Ok(window.pop_event(access)))
        .unwrap()
        .expect("the click bubbles to the window");
    assert_eq!(event.code, EventKind::Clicked);
    assert!(event.is_target(child));
    assert!(!event.is_target(window_object));
    let found = manager
        .lock_function_sync(|| Ok(unsafe { event.find_target_in(window_object) }))
        .unwrap();
    assert_eq!(found.map(|object| object.as_ptr()), Some(child));
    manager
        .with_lvgl_sync(|access| {
            access
                .edit(|access| access.screen_active()?.child(0)?.child(0))
                .unwrap()
                .delete()?;
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, window_object), 0);
    // A copy taken before the deletion still compares equal, but its target
    // is no longer found alive.
    assert!(event.is_target(child));
    let found = manager
        .lock_function_sync(|| Ok(unsafe { event.find_target_in(window_object) }))
        .unwrap();
    assert_eq!(found, None);

    // Inside a window parent, the window protects every ancestor below the
    // screen too.
    let body = manager
        .lock_function_sync(|| Ok(unsafe { lvgl::lv_obj_create(screen) }))
        .unwrap();
    // SAFETY: `body` stays alive until the window inside it is deleted.
    manager
        .with_lvgl_sync(|access| {
            // SAFETY: body stays live while it is the window parent.
            unsafe { manager.set_window_parent(access, body) };
            Ok(())
        })
        .unwrap();
    let nested = manager
        .with_lvgl_sync(|access| manager.create_window(access))
        .unwrap();
    assert_eq!(child_count(manager, body), 1);
    let deleted_body = manager
        .with_lvgl_sync(|access| {
            Ok(access
                .edit(|access| access.screen_active()?.child(1))
                .unwrap()
                .delete())
        })
        .unwrap();
    assert!(
        deleted_body.is_err(),
        "deleting the parent must spare the window"
    );
    assert_eq!(child_count(manager, body), 1);

    // Only the owners delete their windows, once: in a session, or on drop.
    manager
        .with_lvgl_sync(|access| {
            nested.delete(access);
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, body), 0);
    drop(window);
    assert_eq!(child_count(manager, screen), 1);
    // Without windows, the parent is an ordinary object again.
    manager
        .with_lvgl_sync(|access| {
            // SAFETY: the manager keeps the screen live.
            unsafe { manager.set_window_parent(access, screen) };
            access
                .edit(|access| access.screen_active()?.child(0))
                .unwrap()
                .delete()?;
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, screen), 0);

    // The public close request uses the lock for lookup and dispatch.
    let mut window = manager
        .with_lvgl_sync(|access| manager.create_window(access))
        .unwrap();
    let identifier = window.get_identifier();
    window.register_waker(core::task::Waker::noop());
    window.wake_up();
    let mut guard = manager.synchronous_lock();
    guard
        .with_lvgl(|access| {
            while window.pop_event(access).is_some() {}
            manager
                .send_window_close_request(access, identifier)
                .unwrap();
            // SAFETY: this session holds the lock and the owner keeps the window live.
            let raw_window = unsafe { graphics::Window::from_raw(window.as_object()).unwrap() };
            // SAFETY: the same session and owner still protect the queue.
            let pending = unsafe { graphics::Window::peek_event(raw_window) };
            assert_eq!(pending.unwrap().code, EventKind::CloseRequested);
            assert_eq!(
                window.pop_event(access).unwrap().code,
                EventKind::CloseRequested
            );
            drop(window);
        })
        .unwrap();
    // A drop during a session queues deletion instead of blocking. A
    // subsequent session drains it before it can resolve this identifier.
    guard
        .with_lvgl(|access| {
            assert_eq!(manager.get_window_count(access).unwrap(), 0);
            assert_eq!(
                manager.send_window_close_request(access, identifier),
                Err(graphics::Error::InvalidWindowIdentifier),
            );
        })
        .unwrap();
}
