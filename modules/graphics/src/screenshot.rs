use alloc::vec::Vec;

use crate::{Error, Result, lvgl};

/// Maximum active-screen area accepted by [`crate::Manager::capture_screen`].
pub const SCREEN_SNAPSHOT_MAX_PIXELS: usize = 2_073_600;

/// An owned RGBA8888 copy of the active screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenSnapshot {
    pub width: u16,
    pub height: u16,
    pub pixels_rgba: Vec<u8>,
}

fn copy_strided_bgra_to_rgba(
    width: usize,
    height: usize,
    stride: usize,
    source: &[u8],
) -> Result<Vec<u8>> {
    let pixel_count = width.checked_mul(height).ok_or(Error::InvalidParameter)?;
    let row_bytes = width.checked_mul(4).ok_or(Error::InvalidParameter)?;
    let source_len = stride.checked_mul(height).ok_or(Error::InvalidParameter)?;
    if width == 0
        || height == 0
        || pixel_count > SCREEN_SNAPSHOT_MAX_PIXELS
        || stride < row_bytes
        || source.len() < source_len
    {
        return Err(Error::InvalidParameter);
    }

    let output_len = pixel_count.checked_mul(4).ok_or(Error::InvalidParameter)?;
    let mut pixels_rgba = Vec::new();
    pixels_rgba
        .try_reserve_exact(output_len)
        .map_err(|_| Error::OutOfMemory)?;
    for row in 0..height {
        let row_start = row * stride;
        let (row_pixels, _) = source[row_start..row_start + row_bytes].as_chunks::<4>();
        for pixel in row_pixels {
            // LVGL's ARGB8888 draw-buffer bytes are B, G, R, A on the
            // little-endian targets supported by the native renderer.
            pixels_rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    Ok(pixels_rgba)
}

struct DrawBuffer(*mut lvgl::lv_draw_buf_t);

impl Drop for DrawBuffer {
    fn drop(&mut self) {
        unsafe { lvgl::lv_draw_buf_destroy(self.0) };
    }
}

pub(crate) fn capture_screen(object: *mut lvgl::lv_obj_t) -> Result<ScreenSnapshot> {
    if object.is_null() {
        return Err(Error::NotAvailable);
    }

    // LVGL allocates the complete snapshot inside lv_snapshot_take. Check its
    // layout-derived dimensions first so an oversized object cannot trigger
    // an unbounded allocation before our post-capture validation.
    let (expected_width, expected_height) = unsafe {
        lvgl::lv_obj_update_layout(object);
        let width = lvgl::lv_obj_get_width(object);
        let height = lvgl::lv_obj_get_height(object);
        let ext = lvgl::lv_obj_get_ext_draw_size(object);
        let twice_ext = ext.checked_mul(2).ok_or(Error::InvalidParameter)?;
        (
            width
                .checked_add(twice_ext)
                .ok_or(Error::InvalidParameter)?,
            height
                .checked_add(twice_ext)
                .ok_or(Error::InvalidParameter)?,
        )
    };
    if expected_width <= 0
        || expected_height <= 0
        || (expected_width as usize)
            .checked_mul(expected_height as usize)
            .is_none_or(|area| area > SCREEN_SNAPSHOT_MAX_PIXELS)
    {
        return Err(Error::InvalidParameter);
    }

    let draw_buf =
        unsafe { lvgl::snapshot_take(object, lvgl::lv_color_format_t_LV_COLOR_FORMAT_ARGB8888) };
    if draw_buf.is_null() {
        return Err(Error::OutOfMemory);
    }
    let draw_buf = DrawBuffer(draw_buf);

    let header = unsafe { &(*draw_buf.0).header };
    let width = header.w() as usize;
    let height = header.h() as usize;
    let pixel_count = width.checked_mul(height).ok_or(Error::InvalidParameter)?;
    if width == 0 || height == 0 || pixel_count > SCREEN_SNAPSHOT_MAX_PIXELS {
        return Err(Error::InvalidParameter);
    }

    let width_u16 = u16::try_from(width).map_err(|_| Error::InvalidParameter)?;
    let height_u16 = u16::try_from(height).map_err(|_| Error::InvalidParameter)?;
    let row_bytes = width.checked_mul(4).ok_or(Error::InvalidParameter)?;
    let stride = header.stride() as usize;
    let source_len = stride.checked_mul(height).ok_or(Error::InvalidParameter)?;
    let source = unsafe { &*draw_buf.0 };
    if header.cf() != lvgl::lv_color_format_t_LV_COLOR_FORMAT_ARGB8888
        || stride < row_bytes
        || source.data.is_null()
        || (source.data_size as usize) < source_len
    {
        return Err(Error::InvalidParameter);
    }

    let source_bytes = unsafe { core::slice::from_raw_parts(source.data, source_len) };
    let pixels_rgba = copy_strided_bgra_to_rgba(width, height, stride, source_bytes)?;

    Ok(ScreenSnapshot {
        width: width_u16,
        height: height_u16,
        pixels_rgba,
    })
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn screen_snapshot_copies_strided_rgba_pixels() {
        let source = [3, 2, 1, 4, 0, 0, 0, 0, 30, 20, 10, 40, 0, 0, 0, 0];
        assert_eq!(
            copy_strided_bgra_to_rgba(1, 2, 8, &source),
            Ok(vec![1, 2, 3, 4, 10, 20, 30, 40])
        );
    }

    #[test]
    fn screen_snapshot_rejects_malformed_draw_buffer() {
        assert_eq!(
            copy_strided_bgra_to_rgba(2, 1, 7, &[0; 8]),
            Err(Error::InvalidParameter)
        );
        assert_eq!(
            copy_strided_bgra_to_rgba(2, 1, 8, &[0; 7]),
            Err(Error::InvalidParameter)
        );
    }

    #[test]
    fn screen_snapshot_rejects_dimensions_over_limit() {
        assert_eq!(
            copy_strided_bgra_to_rgba(SCREEN_SNAPSHOT_MAX_PIXELS + 1, 1, 0, &[]),
            Err(Error::InvalidParameter)
        );
    }
}
