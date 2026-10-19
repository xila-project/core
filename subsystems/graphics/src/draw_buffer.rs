use alloc::{vec, vec::Vec};

use crate::{Color, ColorRGB888, Point, RenderingColor};

#[repr(transparent)]
pub struct Buffer {
    buffer: Vec<RenderingColor>,
}

impl AsRef<[Color]> for Buffer {
    fn as_ref(&self) -> &[Color] {
        unsafe {
            let buffer = self.buffer.as_ptr() as *const Color;
            core::slice::from_raw_parts(buffer, self.buffer.len())
        }
    }
}

impl Buffer {
    pub fn new_from_resolution(resolution: &Point) -> Self {
        let buffer_size = get_minimal_buffer_size(resolution);

        Self::new(buffer_size)
    }

    pub fn new(buffer_size: usize) -> Self {
        Self {
            buffer: vec![ColorRGB888::new(0, 0, 0).into(); buffer_size],
        }
    }
}

pub const fn get_recommended_buffer_size(resolution: &Point) -> usize {
    resolution.get_x() as usize * resolution.get_y() as usize
}

pub const fn get_minimal_buffer_size(resolution: &Point) -> usize {
    if resolution.get_x() < resolution.get_y() {
        resolution.get_y() as usize * resolution.get_y() as usize / 10
    } else {
        resolution.get_x() as usize * resolution.get_x() as usize / 10
    }
}
