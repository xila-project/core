use file_system::{ControlCommand, define_command};

use crate::{Area, Point};

define_command!(SET_DRAWING_AREA, Write, b'D', 0x01, Area, ());
define_command!(GET_RESOLUTION, Read, b'D', 0x02, (), Point);
define_command!(WAS_RESIZED, Read, b'D', 0x03, (), bool);

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
    b'D',
    0x04,
    ScreenshotRequest,
    ScreenshotDimensions
);
