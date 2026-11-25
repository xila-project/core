//! `Bootsplash::stop` deletes the logo while it holds the graphics lock.
#![cfg(target_os = "linux")]
#![allow(clippy::disallowed_types, reason = "host-only test")]

testing::instantiate_global_allocator!();

use core::ffi::c_void;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::{Context, Poll};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::{GET_RESOLUTION, Manager, Point, SET_DRAWING_AREA, WAS_RESIZED, lvgl};
use shared::AnyByLayout;

/// A screen that accepts every frame.
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

const NOT_DELETED: u8 = 0;
const DELETED_WITH_LOCK: u8 = 1;
const DELETED_WITHOUT_LOCK: u8 = 2;

static DELETION: AtomicU8 = AtomicU8::new(NOT_DELETED);

/// Records whether the graphics lock was held when LVGL deleted the logo.
unsafe extern "C" fn on_delete(_event: *mut lvgl::lv_event_t) {
    let held = graphics::ffi_manager().try_lock().is_none();
    DELETION.store(
        if held {
            DELETED_WITH_LOCK
        } else {
            DELETED_WITHOUT_LOCK
        },
        Ordering::SeqCst,
    );
}

#[test]
fn stop_deletes_the_logo_under_the_graphics_lock() {
    let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let manager: &'static Manager = Box::leak(Box::new(task::block_on(
        graphics::initialize_with_input_devices(time, &Screen, &[], 1024, false),
    )));
    graphics::set_ffi_manager(manager);

    task::block_on(async {
        let bootsplash = bootsplash::Bootsplash::new(manager).await.unwrap();
        {
            let _lock = manager.lock().await;
            unsafe {
                let screen = manager.get_current_screen().unwrap();
                let logo = lvgl::lv_obj_get_child(screen, -1);
                assert!(!logo.is_null());
                lvgl::lv_obj_add_event_cb(
                    logo,
                    Some(on_delete),
                    lvgl::lv_event_code_t_LV_EVENT_DELETE,
                    core::ptr::null_mut::<c_void>(),
                );
            }
        }

        bootsplash.stop(manager).await.unwrap();
    });

    assert_eq!(
        DELETION.load(Ordering::SeqCst),
        DELETED_WITH_LOCK,
        "the logo was not deleted, or was deleted without the graphics lock"
    );
}
