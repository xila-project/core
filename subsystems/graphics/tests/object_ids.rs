//! Guest-visible object identity: per-table monotonic ids kept in LVGL's
//! `id` word, a sorted table per consumer, removal by pointer in the free hook,
//! and event targets that carry the id read when the event was queued.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use core::{
    num::NonZeroU32,
    ptr::NonNull,
    task::{Context, Poll},
};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
#[expect(
    deprecated,
    reason = "the test sends raw LVGL events and writes the raw id"
)]
use graphics::lvgl;
use graphics::{
    Error, GET_RESOLUTION, Point, SET_DRAWING_AREA, WAS_RESIZED,
    lvgl_rust::Obj,
    object_id::{ObjectTable, set_free_observer},
};
use shared::AnyByLayout;
use std::sync::Mutex;

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

/// Two consumers, as two Wasm VMs would be.
struct Tables {
    first: ObjectTable,
    second: ObjectTable,
}

// SAFETY: the test touches the tables only under the graphics lock.
unsafe impl Send for Tables {}

static TABLES: Mutex<Option<Tables>> = Mutex::new(None);

fn freed(object: NonNull<lvgl::lv_obj_t>, id: NonZeroU32) {
    let mut tables = TABLES.lock().unwrap();
    let tables = tables.as_mut().unwrap();
    tables.first.remove(id.get(), object);
    tables.second.remove(id.get(), object);
}

fn with_tables<T>(f: impl FnOnce(&mut Tables) -> T) -> T {
    f(TABLES.lock().unwrap().as_mut().unwrap())
}

#[test]
fn identifiers_follow_their_objects_through_every_deletion_path() {
    let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let manager = Box::leak(Box::new(task::block_on(
        graphics::initialize_with_input_devices(time, &Screen, &[], 1024, false),
    )));
    graphics::set_ffi_manager(manager);
    set_free_observer(freed).unwrap();
    // The same function again is accepted; another one is refused, without
    // claiming the process is busy.
    set_free_observer(freed).unwrap();
    assert_eq!(
        set_free_observer(|_, _| {}),
        Err(Error::FailedToRegister),
        "a second, different observer must be refused"
    );
    *TABLES.lock().unwrap() = Some(Tables {
        first: ObjectTable::new(),
        second: ObjectTable::new(),
    });

    let mut window = task::block_on(manager.create_window()).unwrap();
    manager
        .with_lvgl_sync(|lv| {
            // Five children of the window's content.
            let mut content = window.content_mut(lv);
            let mut pointers = Vec::new();
            for _ in 0..5 {
                pointers.push(NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap());
            }
            drop(content);

            // Each table issues its own ids, starting at 1, and names only the
            // objects it published. Untouched widgets carry no id.
            let ids = with_tables(|t| unsafe {
                [
                    t.first.publish(pointers[0]).unwrap(),
                    t.first.publish(pointers[1]).unwrap(),
                    t.first.publish(pointers[2]).unwrap(),
                    t.second.publish(pointers[3]).unwrap(),
                ]
            });
            assert_eq!(ids.map(NonZeroU32::get), [1, 2, 3, 1]);
            with_tables(|t| {
                assert_eq!(t.first.len(), 3);
                assert_eq!(t.second.len(), 1);
                // Identical numbers name different objects in each table.
                assert_eq!(t.first.find(1), Some(pointers[0]));
                assert_eq!(t.second.find(1), Some(pointers[3]));
                // Foreign, stale and invented values are simply not found.
                assert_eq!(t.first.find(4), None);
                assert_eq!(t.second.find(2), None);
                assert_eq!(t.first.find(0), None);
                assert_eq!(t.first.find(u32::MAX), None);
                // Ids read back, and an object named twice is refused.
                assert_eq!(unsafe { t.first.id_of(pointers[2]) }, Some(ids[2]));
                assert_eq!(unsafe { t.first.id_of(pointers[3]) }, None);
                assert_eq!(unsafe { t.first.id_of(pointers[4]) }, None);
                assert_eq!(
                    unsafe { t.second.publish(pointers[0]) },
                    Err(Error::AlreadyInUse)
                );
                // 8 B per entry on 32-bit targets, in steps of four: never more
                // than one step beyond the content.
                let entry = 4 + size_of::<usize>();
                assert!(t.first.capacity_bytes() <= (t.first.len() + 4) * entry);
            });

            // Deleting the second table's object removes its entry by pointer:
            // the first table's id 1 is another object and stays.
            window
                .edit(lv, |root| root.child(3))
                .unwrap()
                .delete()
                .unwrap();
            with_tables(|t| {
                assert_eq!(t.second.len(), 0);
                assert_eq!(t.first.find(1), Some(pointers[0]));
                assert_eq!(t.first.len(), 3);
            });

            // A cascade frees every identified descendant, and `clean` too.
            let parent = NonNull::new({
                let mut content = window.content_mut(lv);
                Obj::create(&mut content).unwrap().as_ptr()
            })
            .unwrap();
            let child = NonNull::new({
                let mut parent_edit = window
                    .edit(lv, |root| root.child(-1))
                    .expect("the new object is the last child");
                Obj::create(&mut parent_edit).unwrap().as_ptr()
            })
            .unwrap();
            let (parent_id, child_id) = with_tables(|t| unsafe {
                (
                    t.first.publish(parent).unwrap(),
                    t.first.publish(child).unwrap(),
                )
            });
            assert_eq!((parent_id.get(), child_id.get()), (4, 5));
            assert_eq!(with_tables(|t| t.first.len()), 5);
            window
                .edit(lv, |root| root.child(-1))
                .unwrap()
                .delete()
                .unwrap();
            with_tables(|t| {
                assert_eq!(t.first.find(4), None);
                assert_eq!(t.first.find(5), None);
                assert_eq!(t.first.len(), 3);
            });

            // Identifiers are never reissued: the next one is 6, whatever
            // address LVGL hands out.
            let again = NonNull::new({
                let mut content = window.content_mut(lv);
                Obj::create(&mut content).unwrap().as_ptr()
            })
            .unwrap();
            let id = with_tables(|t| unsafe { t.first.publish(again) }).unwrap();
            assert_eq!(id.get(), 6);
            window.content_mut(lv).clean().unwrap();
            with_tables(|t| assert_eq!(t.first.len(), 0));
            assert!(window.view(lv).child(0).is_none());
            Ok(())
        })
        .unwrap();

    // Event targets carry the id read when the event was queued.
    let first_id = manager
        .with_lvgl_sync(|lv| {
            let mut content = window.content_mut(lv);
            let target = NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap();
            let native = NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap();
            drop(content);
            let id = with_tables(|t| unsafe { t.first.publish(target) }).unwrap();
            while window.pop_event().is_some() {}
            // SAFETY: the graphics lock is held and both objects are live.
            unsafe {
                lvgl::lv_obj_send_event(
                    target.as_ptr(),
                    lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                    core::ptr::null_mut(),
                );
                lvgl::lv_obj_send_event(
                    native.as_ptr(),
                    lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                    core::ptr::null_mut(),
                );
            }
            let named = window.pop_event().unwrap();
            let unnamed = window.pop_event().unwrap();
            assert_eq!(named.target_id(), Some(id));
            assert!(named.is_target(target.as_ptr()));
            assert_eq!(unnamed.target_id(), None, "an unpublished widget has no id");
            // The queued event outlives its target: its id names nothing.
            window
                .edit(lv, |root| root.child(-2))
                .unwrap()
                .delete()
                .unwrap();
            with_tables(|t| assert_eq!(t.first.find(id.get()), None));
            Ok(id)
        })
        .unwrap();
    manager
        .with_lvgl_sync(|lv| {
            // A new widget, possibly at the freed address, gets a new id.
            let mut content = window.content_mut(lv);
            let replacement = NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap();
            let id = with_tables(|t| unsafe { t.first.publish(replacement) }).unwrap();
            assert!(id > first_id);
            with_tables(|t| assert_eq!(t.first.find(first_id.get()), None));

            // LVGL's own setter cannot name or rename objects, only untrack.
            unsafe {
                lvgl::lv_obj_set_id(replacement.as_ptr(), 77 as *mut core::ffi::c_void);
                assert_eq!(
                    t_id(&replacement),
                    id.get(),
                    "a nonzero write through LVGL's API is ignored"
                );
                lvgl::lv_obj_set_id(replacement.as_ptr(), core::ptr::null_mut());
                assert_eq!(t_id(&replacement), 0);
            }
            with_tables(|t| assert_eq!(t.first.find(id.get()), None));
            Ok(())
        })
        .unwrap();

    // A table that ran out of identifiers fails only its own creations.
    manager
        .with_lvgl_sync(|lv| {
            let mut content = window.content_mut(lv);
            let a = NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap();
            let b = NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap();
            let c = NonNull::new(Obj::create(&mut content).unwrap().as_ptr()).unwrap();
            let mut exhausted = ObjectTable::with_last_issued(u32::MAX - 1);
            assert_eq!(unsafe { exhausted.publish(a) }.unwrap().get(), u32::MAX);
            assert_eq!(unsafe { exhausted.publish(b) }, Err(Error::NotAvailable));
            assert_eq!(exhausted.len(), 1);
            // The failed object stays usable and unnamed; another table works.
            assert!(with_tables(|t| unsafe { t.second.publish(b) }).is_ok());
            // Free the exhausted table's object before the table goes away.
            let _ = exhausted.take_objects();
            let _ = c;
            Ok(())
        })
        .unwrap();

    drop(window);
}

fn t_id(object: &NonNull<lvgl::lv_obj_t>) -> u32 {
    // SAFETY: the object is live and the graphics lock is held.
    unsafe { lvgl::lv_obj_get_id(object.as_ptr()) }.addr() as u32
}
