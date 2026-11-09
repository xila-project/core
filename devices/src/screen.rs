//! Screen device control contract (`device::kind::SCREEN`).
//!
//! Screen devices expose resolution queries, resize notifications, drawing-area
//! selection before pixel writes, and optional screenshot capture via
//! `File::control`.

use file_system::{ControlCommand, define_command};

use crate::kind;

/// A 2D coordinate or resolution (`x`, `y`) in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }

    pub const fn get_x(&self) -> i16 {
        self.x
    }

    pub const fn get_y(&self) -> i16 {
        self.y
    }

    pub fn split(self) -> (i16, i16) {
        (self.x, self.y)
    }

    pub fn set_x(mut self, value: i16) -> Self {
        self.x = value;
        self
    }

    pub fn set_y(mut self, value: i16) -> Self {
        self.y = value;
        self
    }

    pub fn set(mut self, x: i16, y: i16) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    pub const fn scale(mut self, factor: f64) -> Self {
        let x = self.x as f64 * factor;
        let y = self.y as f64 * factor;
        self.x = (if x >= 0.0 { x + 0.5 } else { x - 0.5 }) as i16;
        self.y = (if y >= 0.0 { y + 0.5 } else { y - 0.5 }) as i16;
        self
    }
}

impl From<(i16, i16)> for Point {
    fn from((x, y): (i16, i16)) -> Self {
        Self::new(x, y)
    }
}

impl From<Point> for (i16, i16) {
    fn from(point: Point) -> Self {
        point.split()
    }
}

/// Inclusive rectangular drawing area defined by two corner [`Point`]s.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Area(Point, Point);

impl Area {
    pub fn new(point_1: Point, point_2: Point) -> Self {
        Self(point_1, point_2)
    }

    pub fn get_point_1(&self) -> Point {
        self.0
    }

    pub fn get_point_2(&self) -> Point {
        self.1
    }

    pub fn get_width(&self) -> u16 {
        (self.1.get_x() - self.0.get_x()).unsigned_abs() + 1
    }

    pub fn get_height(&self) -> u16 {
        (self.1.get_y() - self.0.get_y()).unsigned_abs() + 1
    }

    pub fn set_point_1(mut self, value: Point) -> Self {
        self.0 = value;
        self
    }

    pub fn set_point_2(mut self, value: Point) -> Self {
        self.1 = value;
        self
    }
}

impl AsRef<[u8]> for Area {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const _ as *const u8, core::mem::size_of::<Self>())
        }
    }
}

impl AsRef<Area> for [u8; core::mem::size_of::<Area>()] {
    fn as_ref(&self) -> &Area {
        unsafe { &*(self as *const _ as *const Area) }
    }
}

define_command!(SET_DRAWING_AREA, Write, kind::SCREEN, 0x01, Area, ());
define_command!(GET_RESOLUTION, Read, kind::SCREEN, 0x02, (), Point);
define_command!(WAS_RESIZED, Read, kind::SCREEN, 0x03, (), bool);

pub const SCREENSHOT_PATH_MAX_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ScreenshotDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct ScreenshotRequest {
    path: [u8; SCREENSHOT_PATH_MAX_BYTES],
    length: u16,
}

impl ScreenshotRequest {
    pub fn new(path: &str) -> Option<Self> {
        let path = path.as_bytes();
        if path.is_empty() || path.len() > SCREENSHOT_PATH_MAX_BYTES {
            return None;
        }

        let mut request = Self {
            path: [0; SCREENSHOT_PATH_MAX_BYTES],
            length: path.len() as u16,
        };
        request.path[..path.len()].copy_from_slice(path);
        Some(request)
    }

    pub fn path(&self) -> Option<&str> {
        core::str::from_utf8(&self.path[..self.length as usize]).ok()
    }
}

define_command!(
    WRITE_SCREENSHOT_TO_PATH,
    Write,
    kind::SCREEN,
    0x04,
    ScreenshotRequest,
    ScreenshotDimensions
);
