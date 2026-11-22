use alloc::{vec, vec::Vec};

use crate::{ColorRGB888, Point, RenderingColor};

/// An LVGL draw buffer: `len` pixels in the rendering colour format.
#[repr(transparent)]
pub struct Buffer {
    buffer: Vec<RenderingColor>,
}

impl AsRef<[RenderingColor]> for Buffer {
    fn as_ref(&self) -> &[RenderingColor] {
        &self.buffer
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

    /// Number of pixels.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Size of the buffer in bytes, as LVGL renders into it.
    pub fn size_in_bytes(&self) -> usize {
        size_of_val(self.buffer.as_slice())
    }

    /// The start of the buffer, for LVGL to render into. The allocation does
    /// not move when the `Buffer` does.
    pub fn as_mut_ptr(&mut self) -> *mut RenderingColor {
        self.buffer.as_mut_ptr()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_cover_exactly_the_allocated_pixels() {
        let buffer = Buffer::new(19_200);
        assert_eq!(buffer.len(), 19_200);
        // The buffer holds rendering pixels: a view typed with another colour
        // size (such as 3-byte `ColorRGB888`) would read past or short of the
        // allocation.
        assert_eq!(
            size_of_val(buffer.as_ref()),
            19_200 * size_of::<RenderingColor>()
        );
        assert_eq!(buffer.size_in_bytes(), size_of_val(buffer.as_ref()));
    }
}
