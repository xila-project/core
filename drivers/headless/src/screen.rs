use core::fmt;

use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
    MountOperations, Size,
};
use graphics::{
    Area, GET_RESOLUTION, Point, RenderingColor, SCREEN_SNAPSHOT_MAX_PIXELS, SET_DRAWING_AREA,
    WAS_RESIZED,
};
use shared::{AnyByLayout, align_slice_to};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadlessError {
    InvalidResolution,
}

impl fmt::Display for HeadlessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidResolution => formatter.write_str("invalid headless resolution"),
        }
    }
}

pub struct HeadlessScreenDevice {
    resolution: Point,
    drawing_area: Mutex<Area>,
}

impl HeadlessScreenDevice {
    pub fn new(resolution: Point) -> Result<Self, HeadlessError> {
        validate_resolution(resolution).map_err(|_| HeadlessError::InvalidResolution)?;
        Ok(Self {
            resolution,
            drawing_area: Mutex::new(Area::default()),
        })
    }
}

impl DirectBaseOperations for HeadlessScreenDevice {
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
            let pixels: &[RenderingColor] =
                align_slice_to(buffer).ok_or(file_system::Error::InvalidParameter)?;
            let expected = self
                .drawing_area
                .lock()
                .map_err(|_| file_system::Error::InputOutput)
                .and_then(|area| {
                    validate_flush(self.resolution, *area, size_of::<RenderingColor>())
                        .map_err(|_| file_system::Error::InvalidParameter)
                })?;
            if pixels.len().checked_mul(size_of::<RenderingColor>()) != Some(expected)
                || buffer.len() != expected
            {
                return Err(file_system::Error::InvalidParameter);
            }
            Ok(buffer.len())
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
                    let area = *SET_DRAWING_AREA::cast_input(input)?;
                    *self
                        .drawing_area
                        .lock()
                        .map_err(|_| file_system::Error::InputOutput)? = area;
                }
                GET_RESOLUTION::IDENTIFIER => {
                    *GET_RESOLUTION::cast_output(output)? = self.resolution;
                }
                WAS_RESIZED::IDENTIFIER => {
                    *WAS_RESIZED::cast_output(output)? = false;
                }
                _ => return Err(file_system::Error::UnsupportedOperation),
            }
            Ok(())
        })
    }
}

impl MountOperations for HeadlessScreenDevice {}
impl DirectCharacterDevice for HeadlessScreenDevice {}

fn validate_resolution(resolution: Point) -> Result<(), ()> {
    let width = usize::try_from(resolution.get_x()).map_err(|_| ())?;
    let height = usize::try_from(resolution.get_y()).map_err(|_| ())?;
    if width == 0
        || height == 0
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > SCREEN_SNAPSHOT_MAX_PIXELS)
    {
        Err(())
    } else {
        Ok(())
    }
}

fn validate_flush(resolution: Point, area: Area, bytes_per_pixel: usize) -> Result<usize, ()> {
    let first = area.get_point_1();
    let second = area.get_point_2();
    let (x1, y1, x2, y2) = (
        i32::from(first.get_x()),
        i32::from(first.get_y()),
        i32::from(second.get_x()),
        i32::from(second.get_y()),
    );
    let width = i32::from(resolution.get_x());
    let height = i32::from(resolution.get_y());
    if x1 < 0 || y1 < 0 || x2 < x1 || y2 < y1 || x2 >= width || y2 >= height {
        return Err(());
    }
    let pixels = usize::try_from(x2 - x1 + 1)
        .ok()
        .and_then(|flush_width| {
            usize::try_from(y2 - y1 + 1)
                .ok()
                .and_then(|flush_height| flush_width.checked_mul(flush_height))
        })
        .ok_or(())?;
    pixels.checked_mul(bytes_per_pixel).ok_or(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_screen_accepts_valid_flush() {
        let resolution = Point::new(640, 480);
        let area = Area::new(Point::new(4, 8), Point::new(15, 19));
        assert_eq!(validate_flush(resolution, area, 4), Ok(12 * 12 * 4));
    }

    #[test]
    fn headless_screen_rejects_invalid_dimensions() {
        assert_eq!(validate_resolution(Point::new(0, 480)), Err(()));
        assert_eq!(validate_resolution(Point::new(1920, 1081)), Err(()));
    }

    #[test]
    fn headless_screen_rejects_out_of_bounds_flush() {
        let area = Area::new(Point::new(639, 0), Point::new(640, 1));
        assert_eq!(validate_flush(Point::new(640, 480), area, 4), Err(()));
    }
}
