//! A window belongs to its `OwnedWindow`: safe code can edit its content but
//! never delete it, and queued event targets are never dereferenced after
//! safe code deleted them (xila-project/core#661).
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use core::task::{Context, Poll};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::lvgl_rust::{self, Obj, ScreenAnim, ScreenLoadAnim};
use graphics::{EventKind, GET_RESOLUTION, Manager, Point, SET_DRAWING_AREA, WAS_RESIZED, lvgl};
use shared::AnyByLayout;

struct Screen;
impl MountOperations for Screen {}
impl DirectCharacterDevice for Screen {}
impl DirectBaseOperations for Screen {
    fn read(&self, _: &mut Context<'_>, _: &mut [u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::Error::UnsupportedOperation))
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
            WAS_RESIZED::IDENTIFIER => {
                WAS_RESIZED::cast_output(output).map(|resized| *resized = false)
            }
            SET_DRAWING_AREA::IDENTIFIER => Ok(()),
            _ => Err(file_system::Error::UnsupportedOperation),
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
        .with_lvgl_sync(|lv| {
            // The issue's reproduction: no `unsafe`.
            let delete = lv.edit(|lv| lv.screen_active()?.child(0)).unwrap().delete();
            let delete_later = lv.screen_active().unwrap().child(0).unwrap().delete_later();
            let move_to_index = lv
                .edit(|lv| lv.screen_active()?.child(0))
                .unwrap()
                .move_to_index(-1);
            let set_parent = lv
                .edit(|lv| lv.screen_active()?.child(0))
                .unwrap()
                .set_parent_to(|lv| lv.screen_active());
            let clean_screen = lv.screen_active_mut().unwrap().clean();
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
    let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let manager: &'static Manager = Box::leak(Box::new(task::block_on(
        graphics::initialize_with_input_devices(time, &Screen, &[], 1024, false),
    )));
    // `OwnedWindow::drop` takes the graphics lock through the FFI manager.
    graphics::set_ffi_manager(manager);
    let screen = manager.get_current_screen().unwrap();

    // A top-level window: its only ancestor is the screen, a plain object.
    let mut window = task::block_on(manager.create_window()).unwrap();
    let window_object = window.as_object_mutable();
    assert_eq!(child_count(manager, screen), 1);
    for (operation, result) in ["delete", "delete later", "move", "reparent", "clean"]
        .into_iter()
        .zip(safe_edits_of_first_child(manager))
    {
        assert!(result.is_err(), "safe {operation} must spare the window");
    }
    // Deferred requests run in the timer handler: none may delete it.
    manager
        .with_lvgl_sync(|lv| {
            lv.timer_handler();
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, screen), 1);
    // A screen load cannot delete the screen holding the window either.
    let loaded = manager
        .with_lvgl_sync(|lv| {
            let other = lv.create_screen()?;
            let auto_delete = ScreenAnim::new(ScreenLoadAnim::NONE, 0, 0, true);
            Ok(lv.load_screen(other.handle(), auto_delete))
        })
        .unwrap();
    assert!(
        loaded.is_err(),
        "auto-deleting the screen must spare the window"
    );

    // Safe code may build and delete the window's content.
    let child = manager
        .with_lvgl_sync(|lv| {
            let mut window = lv.edit(|lv| lv.screen_active()?.child(0)).unwrap();
            assert_eq!(window.view().as_ptr(), window_object);
            Ok(Obj::create(&mut window)?.as_ptr())
        })
        .unwrap();
    assert_eq!(child_count(manager, window_object), 1);

    // An event for the child is queued, then safe code deletes the child.
    while window.pop_event().is_some() {}
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
    let event = window.pop_event().expect("the click bubbles to the window");
    assert_eq!(event.code, EventKind::Clicked);
    assert!(event.is_target(child));
    assert!(!event.is_target(window_object));
    let found = manager
        .lock_function_sync(|| Ok(unsafe { event.find_target_in(window_object) }))
        .unwrap();
    assert_eq!(found.map(|object| object.as_ptr()), Some(child));
    manager
        .with_lvgl_sync(|lv| {
            lv.edit(|lv| lv.screen_active()?.child(0)?.child(0))
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
    task::block_on(unsafe { manager.set_window_parent(body) }).unwrap();
    let nested = task::block_on(manager.create_window()).unwrap();
    assert_eq!(child_count(manager, body), 1);
    let deleted_body = manager
        .with_lvgl_sync(|lv| Ok(lv.edit(|lv| lv.screen_active()?.child(1)).unwrap().delete()))
        .unwrap();
    assert!(
        deleted_body.is_err(),
        "deleting the parent must spare the window"
    );
    assert_eq!(child_count(manager, body), 1);

    // Only the owners delete their windows, once: in a session, or on drop.
    manager
        .with_lvgl_sync(|lv| {
            nested.delete(lv);
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, body), 0);
    drop(window);
    assert_eq!(child_count(manager, screen), 1);
    // Without windows, the parent is an ordinary object again.
    manager
        .with_lvgl_sync(|lv| {
            lv.edit(|lv| lv.screen_active()?.child(0))
                .unwrap()
                .delete()?;
            Ok(())
        })
        .unwrap();
    assert_eq!(child_count(manager, screen), 0);
}
