use alloc::{vec, vec::Vec};
use core::slice;

use crate::{ColorRGB888, Point, RenderingColor};

/// Alignment of the start of a draw buffer, in bytes. LVGL refuses buffers
/// that are not aligned to `LV_DRAW_BUF_ALIGN` (`include/lv_conf.h`), which
/// must not exceed this.
pub const DRAW_BUFFER_ALIGNMENT: usize = 4;

/// A unit of draw buffer storage: its alignment gives the buffer
/// [`DRAW_BUFFER_ALIGNMENT`] whatever the rendering colour, whose own
/// alignment may be smaller (2 bytes for RGB565).
#[derive(Clone, Copy)]
#[repr(C, align(4))]
struct Storage([u8; DRAW_BUFFER_ALIGNMENT]);

const _: () = {
    assert!(align_of::<Storage>() == DRAW_BUFFER_ALIGNMENT);
    assert!(align_of::<RenderingColor>() <= DRAW_BUFFER_ALIGNMENT);
};

/// An LVGL draw buffer: `len` pixels in the rendering colour format, aligned
/// to [`DRAW_BUFFER_ALIGNMENT`].
pub struct Buffer {
    storage: Vec<Storage>,
    pixels: usize,
}

impl AsRef<[RenderingColor]> for Buffer {
    fn as_ref(&self) -> &[RenderingColor] {
        // SAFETY: the storage is aligned for `RenderingColor` (checked above),
        // holds at least `pixels` of them (`new`) and is initialised.
        unsafe { slice::from_raw_parts(self.storage.as_ptr().cast(), self.pixels) }
    }
}

impl Buffer {
    pub fn new_from_resolution(resolution: &Point) -> Self {
        let buffer_size = get_minimal_buffer_size(resolution);

        Self::new(buffer_size)
    }

    /// A buffer of `buffer_size` black pixels.
    pub fn new(buffer_size: usize) -> Self {
        let bytes = buffer_size
            .checked_mul(size_of::<RenderingColor>())
            .expect("draw buffer size overflows");
        let mut buffer = Self {
            storage: vec![
                Storage([0; DRAW_BUFFER_ALIGNMENT]);
                bytes.div_ceil(DRAW_BUFFER_ALIGNMENT)
            ],
            pixels: buffer_size,
        };
        // SAFETY: as in `as_ref`, with the exclusive borrow of the storage.
        let pixels: &mut [RenderingColor] =
            unsafe { slice::from_raw_parts_mut(buffer.storage.as_mut_ptr().cast(), buffer.pixels) };
        pixels.fill(ColorRGB888::new(0, 0, 0).into());
        buffer
    }

    /// Number of pixels.
    pub fn len(&self) -> usize {
        self.pixels
    }

    pub fn is_empty(&self) -> bool {
        self.pixels == 0
    }

    /// Size of the pixels in bytes, as LVGL renders into them.
    pub fn size_in_bytes(&self) -> usize {
        size_of_val(self.as_ref())
    }

    /// The start of the buffer, for LVGL to render into. It is aligned to
    /// [`DRAW_BUFFER_ALIGNMENT`], and the allocation does not move when the
    /// `Buffer` does.
    pub fn as_mutable_pointer(&mut self) -> *mut RenderingColor {
        self.storage.as_mut_ptr().cast()
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

    #[test]
    fn buffers_are_aligned_for_lvgl() {
        // Odd sizes too: the allocation is rounded up to whole storage units.
        for pixels in [1, 3, 240, 19_201] {
            let mut buffer = Buffer::new(pixels);
            assert!(
                (buffer.as_mutable_pointer() as usize).is_multiple_of(DRAW_BUFFER_ALIGNMENT),
                "{pixels} pixels"
            );
            assert_eq!(buffer.as_ref().len(), pixels);
        }
    }

    #[test]
    fn alignment_covers_lvgl_configuration() {
        let configuration = include_str!("../include/lv_conf.h");
        let alignment: usize = configuration
            .lines()
            .find_map(|line| line.trim().strip_prefix("#define LV_DRAW_BUF_ALIGN "))
            .expect("LV_DRAW_BUF_ALIGN is defined")
            .trim()
            .parse()
            .expect("LV_DRAW_BUF_ALIGN is a number");
        assert!(alignment <= DRAW_BUFFER_ALIGNMENT);
        assert!(DRAW_BUFFER_ALIGNMENT.is_multiple_of(alignment));
    }
}
