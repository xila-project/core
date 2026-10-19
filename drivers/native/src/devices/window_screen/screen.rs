use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::{
    Area, GET_RESOLUTION, RenderingColor, SET_DRAWING_AREA, WAS_RESIZED, WRITE_SCREENSHOT_TO_PATH,
};
use shared::{AnyByLayout, align_slice_to};

use crate::window_screen::inner_window::InnerWindow;

pub struct ScreenDevice<'a>(&'a InnerWindow);

unsafe impl<'a> Sync for ScreenDevice<'a> {}

unsafe impl<'a> Send for ScreenDevice<'a> {}

impl<'a> ScreenDevice<'a> {
    pub fn new(inner: &'a InnerWindow) -> Self {
        Self(inner)
    }
}

impl<'a> DirectBaseOperations for ScreenDevice<'a> {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| Err(file_system::Error::UnsupportedOperation))
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let data: &[RenderingColor] =
                align_slice_to(buffer).ok_or(file_system::Error::InvalidParameter)?;

            task::block_on(self.0.draw(data)).unwrap();

            Ok(size_of::<Self>() as _)
        })
    }

    fn control(
        &self,
        _cx: &mut core::task::Context<'_>,
        command: ControlCommandIdentifier,
        input: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        file_system::ready(|| {
            match command {
                SET_DRAWING_AREA::IDENTIFIER => {
                    let area: &Area = SET_DRAWING_AREA::cast_input(input)?;

                    task::block_on(self.0.set_drawing_area(*area)).unwrap();
                }
                GET_RESOLUTION::IDENTIFIER => {
                    let resolution: &mut graphics::Point = GET_RESOLUTION::cast_output(output)?;

                    *resolution = task::block_on(self.0.get_resolution()).unwrap();
                }
                WAS_RESIZED::IDENTIFIER => {
                    let was_resized: &mut bool = WAS_RESIZED::cast_output(output)?;

                    *was_resized = task::block_on(self.0.was_resized()).unwrap();
                }
                WRITE_SCREENSHOT_TO_PATH::IDENTIFIER => {
                    let request = WRITE_SCREENSHOT_TO_PATH::cast_input(input)?;
                    let path = request.path().ok_or(file_system::Error::InvalidParameter)?;
                    let dimensions =
                        task::block_on(super::screenshot::write_png(std::path::Path::new(path)))
                            .map_err(|_| file_system::Error::InputOutput)?;
                    *WRITE_SCREENSHOT_TO_PATH::cast_output(output)? = dimensions;
                }
                _ => return Err(file_system::Error::UnsupportedOperation),
            }

            Ok(())
        })
    }
}

impl MountOperations for ScreenDevice<'_> {}

impl DirectCharacterDevice for ScreenDevice<'_> {}
