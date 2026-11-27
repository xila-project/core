//! Screen brightness goes from the graphics manager and saved appearances to
//! the screen device, and screens without brightness control are left alone.
#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

use core::task::{Context, Poll};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::appearance::{Appearance, MINIMUM_BRIGHTNESS};
use graphics::{
    BRIGHTNESS_FULL, Error, GET_BRIGHTNESS, GET_RESOLUTION, Manager, Point, SET_BRIGHTNESS,
    SET_DRAWING_AREA, WAS_RESIZED,
};
use portable_atomic::{AtomicBool, AtomicU8, Ordering};
use shared::AnyByLayout;

struct Screen {
    supported: AtomicBool,
    level: AtomicU8,
}

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
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> Poll<file_system::Result<()>> {
        let supported = self.supported.load(Ordering::Relaxed);
        Poll::Ready(match command {
            GET_RESOLUTION::IDENTIFIER => GET_RESOLUTION::cast_output(output)
                .map(|resolution| *resolution = Point::new(320, 240)),
            WAS_RESIZED::IDENTIFIER => WAS_RESIZED::cast_output(output).map(|resized| *resized = 0),
            SET_DRAWING_AREA::IDENTIFIER => Ok(()),
            GET_BRIGHTNESS::IDENTIFIER if supported => GET_BRIGHTNESS::cast_output(output)
                .map(|level| *level = self.level.load(Ordering::Relaxed)),
            SET_BRIGHTNESS::IDENTIFIER if supported => SET_BRIGHTNESS::cast_input(input)
                .map(|level| self.level.store(*level, Ordering::Relaxed)),
            _ => Err(file_system::IoErrorKind::Unsupported),
        })
    }
}

static SCREEN: Screen = Screen {
    supported: AtomicBool::new(true),
    level: AtomicU8::new(BRIGHTNESS_FULL),
};

#[test]
fn brightness_reaches_the_screen_and_appearances_keep_a_visible_minimum() {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let manager: &'static Manager = Box::leak(Box::new(task::block_on(
        graphics::initialize_with_input_devices(time, &SCREEN, &[], 1024, false),
    )));

    task::block_on(async {
        assert_eq!(manager.get_brightness().await, Ok(BRIGHTNESS_FULL));
        manager.set_brightness(100).await.unwrap();
        assert_eq!(SCREEN.level.load(Ordering::Relaxed), 100);
        assert_eq!(manager.get_brightness().await, Ok(100));

        // The contract allows turning the backlight off on purpose.
        manager.set_brightness(0).await.unwrap();
        assert_eq!(SCREEN.level.load(Ordering::Relaxed), 0);

        // A saved appearance never darkens the screen below the minimum.
        let appearance = Appearance {
            brightness: 1,
            ..Appearance::default()
        };
        appearance.apply_brightness(manager).await.unwrap();
        assert_eq!(SCREEN.level.load(Ordering::Relaxed), MINIMUM_BRIGHTNESS);
        let appearance = Appearance {
            brightness: 200,
            ..Appearance::default()
        };
        {
            let guard = manager.lock().await;
            appearance.apply(manager, &guard).await.unwrap();
        }
        assert_eq!(SCREEN.level.load(Ordering::Relaxed), 200);

        // Screens without brightness control report it, and appearances
        // still apply on them.
        SCREEN.supported.store(false, Ordering::Relaxed);
        assert_eq!(manager.get_brightness().await, Err(Error::Unsupported));
        assert_eq!(manager.set_brightness(50).await, Err(Error::Unsupported));
        let guard = manager.lock().await;
        Appearance::default().apply(manager, &guard).await.unwrap();
        assert_eq!(SCREEN.level.load(Ordering::Relaxed), 200);
    });
}
