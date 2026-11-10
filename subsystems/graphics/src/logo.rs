//! Xila Monogram symbol drawn with LVGL.
//!
//! Geometry source: `xila-project/brand` `source/symbol.svg` at commit
//! `00d6686` (identity 2.0.0, designed by Martin Legand). The symbol is four
//! rounded diagonal arms around a separate rounded center square.
//!
//! Each arm is rasterized once, at the requested size, from the supplied path
//! into an alpha-only (`A8`) LVGL draw buffer: the shapes need anti-aliased
//! diagonals and curves that LVGL rectangles cannot express, and the vector
//! renderer (ThorVG) is not built. Only the top-right arm is rasterized; the
//! other arms are exact mirror copies. The center square is a regular rounded
//! LVGL object. A logo of `32 * factor` pixels costs about `0.82 * size²`
//! bytes of heap (3.3 KiB at 64 px, 21 KiB at 160 px) and no flash assets.
//!
//! The axis-aligned edges (center square, arm inner edges and notches) are
//! snapped to whole pixels before rasterization, so the symbol stays crisp and
//! its separations stay open at every factor.

use alloc::vec;

use crate::{Color, Error, Point, Result, lvgl, theme};

/// Width of the supplied artwork, in source units.
const EXTENT: f32 = 617.45;
/// Center of the supplied artwork, in source units.
const CENTER: f32 = EXTENT / 2.0;
/// Distance from the center to the inner edges of an arm.
const ARM_INNER: f32 = CENTER - 280.12;
/// Distance from the center to the outer edges of the notch around the square.
const NOTCH: f32 = 403.63 - CENTER;
/// Half the side of the center square.
const SQUARE_HALF: f32 = 118.02 / 2.0;
/// Corner radius of the center square.
const SQUARE_RADIUS: f32 = 4.4;
/// Line segments used for each cubic Bézier corner.
const CURVE_STEPS: usize = 8;

/// One command of the supplied top-right arm path, in absolute source units.
#[derive(Clone, Copy)]
enum Segment {
    Line(f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
}

/// Top-right arm of `source/symbol.svg`, starting at `M403.63,243.27`.
const ARM_START: (f32, f32) = (403.63, 243.27);
const ARM_PATH: [Segment; 14] = [
    Segment::Line(403.63, 280.12),
    Segment::Line(469.64, 280.12),
    Segment::Cubic(476.30, 280.12, 482.69, 277.47, 487.40, 272.76),
    Segment::Line(610.09, 150.07),
    Segment::Cubic(614.80, 145.36, 617.45, 138.97, 617.45, 132.31),
    Segment::Line(617.45, 25.14),
    Segment::Cubic(617.45, 11.27, 606.20, 0.02, 592.33, 0.02),
    Segment::Line(485.12, 0.02),
    Segment::Cubic(478.46, 0.02, 472.07, 2.67, 467.36, 7.38),
    Segment::Line(344.69, 130.05),
    Segment::Cubic(339.98, 134.76, 337.33, 141.15, 337.33, 147.81),
    Segment::Line(337.33, 213.82),
    Segment::Line(374.18, 213.82),
    Segment::Cubic(390.38, 213.82, 403.63, 227.07, 403.63, 243.27),
];

/// Pixel metrics of a symbol, with the axis-aligned edges snapped to pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Metrics {
    /// Symbol side.
    size: u16,
    /// Distance from the center to the inner edges of an arm.
    arm_inner: u16,
    /// Distance from the center to the outer edges of the notch.
    notch: u16,
    /// Half the side of the center square.
    square_half: u16,
    /// Corner radius of the center square.
    square_radius: u16,
}

impl Metrics {
    fn new(size: u16) -> Self {
        // Keep an even size so the center falls on a pixel boundary.
        let size = size & !1;
        let scale = size as f32 / EXTENT;
        let half = size / 2;
        let snap = |value: f32| (value * scale + 0.5) as u16;

        let arm_inner = snap(ARM_INNER).max(1).min(half);
        let square_half = snap(SQUARE_HALF).max(1).min(half);
        // Keep at least one transparent pixel between the square and the arms.
        let notch = snap(NOTCH).max(square_half + 1).max(arm_inner + 1);

        Self {
            size,
            arm_inner,
            notch,
            square_half,
            square_radius: snap(SQUARE_RADIUS),
        }
    }

    /// Side of the square bitmap holding one arm.
    const fn arm_size(&self) -> u16 {
        self.size / 2 - self.arm_inner
    }

    /// Map a distance from the symbol center in source units to pixels,
    /// keeping the snapped inner, notch and outer edges in place.
    fn map_distance(&self, distance: f32) -> f32 {
        let scale = self.size as f32 / EXTENT;
        let knots = [
            (0.0, 0.0),
            (ARM_INNER, self.arm_inner as f32),
            (NOTCH, self.notch as f32),
            (CENTER, (self.size / 2) as f32),
        ];

        for window in knots.windows(2) {
            let ((from_source, from_pixel), (to_source, to_pixel)) = (window[0], window[1]);

            if distance <= to_source {
                return from_pixel
                    + (distance - from_source) * (to_pixel - from_pixel)
                        / (to_source - from_source);
            }
        }

        (self.size / 2) as f32 + (distance - CENTER) * scale
    }

    /// Map a top-right arm point in source units to its bitmap coordinates.
    fn map_arm_point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let arm_size = self.arm_size() as f32;
        let x = self.map_distance(x - CENTER) - self.arm_inner as f32;
        let y = (self.size / 2) as f32 - self.map_distance(CENTER - y);

        (x.clamp(0.0, arm_size), y.clamp(0.0, arm_size))
    }
}

/// Flatten the top-right arm path into closed polygon vertices, in bitmap pixels.
fn arm_polygon(metrics: &Metrics) -> alloc::vec::Vec<(f32, f32)> {
    let mut points = vec![metrics.map_arm_point(ARM_START)];
    let mut current = ARM_START;

    for segment in ARM_PATH {
        match segment {
            Segment::Line(x, y) => {
                current = (x, y);
                points.push(metrics.map_arm_point(current));
            }
            Segment::Cubic(x1, y1, x2, y2, x, y) => {
                let (x0, y0) = current;
                for step in 1..=CURVE_STEPS {
                    let t = step as f32 / CURVE_STEPS as f32;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    let point = (
                        a * x0 + b * x1 + c * x2 + d * x,
                        a * y0 + b * y1 + c * y2 + d * y,
                    );
                    points.push(metrics.map_arm_point(point));
                }
                current = (x, y);
            }
        }
    }

    points
}

/// Floor of a non-negative value.
fn floor(value: f32) -> f32 {
    (value as i32) as f32
}

/// Accumulate the signed area that one edge covers inside row `[row, row + 1)`.
///
/// This is the exact-area scanline method used by font rasterizers: the prefix
/// sum of `accumulator` gives each pixel's coverage.
fn accumulate_edge(accumulator: &mut [f32], row: f32, from: (f32, f32), to: (f32, f32)) {
    let (direction, (x0, y0), (x1, y1)) = if from.1 < to.1 {
        (1.0, from, to)
    } else if from.1 > to.1 {
        (-1.0, to, from)
    } else {
        return;
    };

    let top = y0.max(row);
    let bottom = y1.min(row + 1.0);
    if top >= bottom {
        return;
    }

    let slope = (x1 - x0) / (y1 - y0);
    let x_top = x0 + (top - y0) * slope;
    let x_bottom = x0 + (bottom - y0) * slope;
    let delta = (bottom - top) * direction;
    let (left, right) = if x_top < x_bottom {
        (x_top, x_bottom)
    } else {
        (x_bottom, x_top)
    };

    let left_floor = floor(left);
    let left_index = left_floor as usize;
    let right_ceil = {
        let floor = floor(right);
        if floor < right { floor + 1.0 } else { floor }
    };
    let right_index = right_ceil as usize;

    if right_index <= left_index + 1 {
        let middle = 0.5 * (x_top + x_bottom) - left_floor;
        accumulator[left_index] += delta - delta * middle;
        accumulator[left_index + 1] += delta * middle;
    } else {
        let inverse_width = 1.0 / (right - left);
        let left_fraction = left - left_floor;
        let first = 0.5 * inverse_width * (1.0 - left_fraction) * (1.0 - left_fraction);
        let right_fraction = right - right_ceil + 1.0;
        let last = 0.5 * inverse_width * right_fraction * right_fraction;

        accumulator[left_index] += delta * first;
        if right_index == left_index + 2 {
            accumulator[left_index + 1] += delta * (1.0 - first - last);
        } else {
            let second = inverse_width * (1.5 - left_fraction);
            accumulator[left_index + 1] += delta * (second - first);
            for value in &mut accumulator[left_index + 2..right_index - 1] {
                *value += delta * inverse_width;
            }
            let before_last = second + (right_index - left_index - 3) as f32 * inverse_width;
            accumulator[right_index - 1] += delta * (1.0 - before_last - last);
        }
        accumulator[right_index] += delta * last;
    }
}

/// Rasterize the top-right arm into an `A8` bitmap of `side × side` pixels.
fn rasterize_arm(metrics: &Metrics, output: &mut [u8], stride: usize) {
    let side = metrics.arm_size() as usize;
    let polygon = arm_polygon(metrics);
    let mut accumulator = vec![0.0f32; side + 2];

    for row in 0..side {
        accumulator.fill(0.0);
        let row_position = row as f32;

        for (index, from) in polygon.iter().enumerate() {
            let to = polygon[(index + 1) % polygon.len()];
            accumulate_edge(&mut accumulator, row_position, *from, to);
        }

        let mut coverage = 0.0f32;
        for (column, alpha) in output[row * stride..row * stride + side]
            .iter_mut()
            .enumerate()
        {
            coverage += accumulator[column];
            let value = if coverage < 0.0 { -coverage } else { coverage };
            *alpha = (value.min(1.0) * 255.0 + 0.5) as u8;
        }
    }
}

/// Keep a one-pixel anti-aliased ring along the edges of a filled bitmap.
fn outline(bitmap: &mut [u8], side: usize, stride: usize) {
    let filled = alloc::vec::Vec::from(&bitmap[..]);
    let at = |x: isize, y: isize| -> u8 {
        if x < 0 || y < 0 || x >= side as isize || y >= side as isize {
            0
        } else {
            filled[y as usize * stride + x as usize]
        }
    };

    for y in 0..side as isize {
        for x in 0..side as isize {
            let eroded = at(x - 1, y)
                .min(at(x + 1, y))
                .min(at(x, y - 1))
                .min(at(x, y + 1));
            bitmap[y as usize * stride + x as usize] = at(x, y).saturating_sub(eroded);
        }
    }
}

/// How the arms and the center square are painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LogoStyle {
    /// Solid symbol, as supplied by the brand.
    #[default]
    Filled,
    /// One-pixel contour of the symbol, for subdued backgrounds.
    Outline,
}

pub struct Logo {
    pub object: *mut lvgl::lv_obj_t,
    buffers: [*mut lvgl::lv_draw_buf_t; Self::ARM_COUNT],
}

unsafe extern "C" fn palette_color_changed(event: *mut lvgl::lv_event_t) {
    unsafe {
        let object = lvgl::lv_event_get_current_target(event) as *mut lvgl::lv_obj_t;
        let role = lvgl::lv_event_get_user_data(event) as usize;

        if let Some(color) = theme::get_palette_color(object, theme::PaletteColor::from_index(role))
        {
            Logo::apply_color(object, color);
        }
    }
}

impl Logo {
    pub const BASE_SIZE: Point = Point::new(32, 32);
    /// Number of arms, created first as children in this order: top-left,
    /// bottom-left, bottom-right, top-right (counter-clockwise).
    pub const ARM_COUNT: usize = 4;
    /// Child index of the center square.
    pub const CENTER_INDEX: usize = Self::ARM_COUNT;

    const ALIGNMENTS: [lvgl::lv_align_t; Self::ARM_COUNT] = [
        lvgl::lv_align_t_LV_ALIGN_TOP_LEFT,
        lvgl::lv_align_t_LV_ALIGN_BOTTOM_LEFT,
        lvgl::lv_align_t_LV_ALIGN_BOTTOM_RIGHT,
        lvgl::lv_align_t_LV_ALIGN_TOP_RIGHT,
    ];

    pub fn get_factor(size: Point) -> u8 {
        let factor_x = size.get_x() / Self::BASE_SIZE.get_x();
        let factor_y = size.get_y() / Self::BASE_SIZE.get_y();

        factor_x.min(factor_y) as u8
    }

    /// Create a filled logo of `32 * factor` pixels.
    ///
    /// # Safety
    /// `parent` must be a live LVGL object and the caller must hold the LVGL lock.
    pub unsafe fn new(parent: *mut lvgl::lv_obj_t, factor: u8, color: Color) -> Result<Self> {
        unsafe { Self::new_with_style(parent, factor, color, LogoStyle::Filled) }
    }

    /// Create a logo of `32 * factor` pixels with the given style.
    ///
    /// # Safety
    /// `parent` must be a live LVGL object and the caller must hold the LVGL lock.
    pub unsafe fn new_with_style(
        parent: *mut lvgl::lv_obj_t,
        factor: u8,
        color: Color,
        style: LogoStyle,
    ) -> Result<Self> {
        if factor == 0 {
            return Err(Error::InvalidParameter);
        }
        let size = (Self::BASE_SIZE.get_x() as u32 * factor as u32).min(u16::MAX as u32) as u16;
        let metrics = Metrics::new(size);

        unsafe {
            let object = lvgl::lv_obj_create(parent);

            if object.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            // Build the logo before handing ownership to `Self`, so a failure
            // releases everything created so far through `Drop`.
            let mut logo = Self {
                object,
                buffers: [core::ptr::null_mut(); Self::ARM_COUNT],
            };

            // Decorative and theme independent: no surface, padding or input.
            lvgl::lv_obj_remove_style_all(object);
            lvgl::lv_obj_set_size(object, metrics.size as i32, metrics.size as i32);
            lvgl::lv_obj_remove_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
            lvgl::lv_obj_remove_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE);

            logo.create_arms(&metrics, style)?;
            logo.create_center(&metrics, style)?;
            Self::apply_color(object, color);

            Ok(logo)
        }
    }

    unsafe fn create_arms(&mut self, metrics: &Metrics, style: LogoStyle) -> Result<()> {
        let side = metrics.arm_size() as u32;

        unsafe {
            for index in 0..Self::ARM_COUNT {
                let buffer = lvgl::lv_draw_buf_create(
                    side,
                    side,
                    lvgl::lv_color_format_t_LV_COLOR_FORMAT_A8,
                    lvgl::LV_STRIDE_AUTO,
                );
                if buffer.is_null() {
                    return Err(Error::OutOfMemory);
                }
                self.buffers[index] = buffer;
            }

            // Every buffer was allocated above with the same geometry; keep the
            // accesses checked so no raw pointer is dereferenced unproven.
            let source_buffer = self.buffers[3].as_mut().ok_or(Error::OutOfMemory)?;
            if source_buffer.data.is_null() {
                return Err(Error::OutOfMemory);
            }
            let stride = source_buffer.header.stride() as usize;
            let length = stride * side as usize;
            let source = core::slice::from_raw_parts_mut(source_buffer.data, length);
            source.fill(0);
            rasterize_arm(metrics, source, stride);
            if style == LogoStyle::Outline {
                outline(source, side as usize, stride);
            }

            // The symbol is mirror symmetric: derive the other arms from the
            // top-right one instead of rasterizing them again.
            let side = side as usize;
            for (index, alignment) in Self::ALIGNMENTS.iter().enumerate().take(3) {
                let (mirror_x, mirror_y) = match *alignment {
                    lvgl::lv_align_t_LV_ALIGN_TOP_LEFT => (true, false),
                    lvgl::lv_align_t_LV_ALIGN_BOTTOM_LEFT => (true, true),
                    _ => (false, true),
                };
                let target_buffer = self.buffers[index].as_mut().ok_or(Error::OutOfMemory)?;
                if target_buffer.data.is_null() {
                    return Err(Error::OutOfMemory);
                }
                let target = core::slice::from_raw_parts_mut(target_buffer.data, length);
                for y in 0..side {
                    let source_y = if mirror_y { side - 1 - y } else { y };
                    for x in 0..side {
                        let source_x = if mirror_x { side - 1 - x } else { x };
                        target[y * stride + x] = source[source_y * stride + source_x];
                    }
                }
            }

            for (buffer, alignment) in self.buffers.iter().zip(Self::ALIGNMENTS) {
                let arm = lvgl::lv_image_create(self.object);
                if arm.is_null() {
                    return Err(Error::FailedToCreateObject);
                }
                lvgl::lv_obj_remove_style_all(arm);
                lvgl::lv_image_set_src(arm, *buffer as *const _);
                lvgl::lv_obj_set_align(arm, alignment);
                lvgl::lv_obj_remove_flag(arm, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
                lvgl::lv_obj_add_flag(arm, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
            }
        }

        Ok(())
    }

    unsafe fn create_center(&mut self, metrics: &Metrics, style: LogoStyle) -> Result<()> {
        unsafe {
            let center = lvgl::lv_obj_create(self.object);
            if center.is_null() {
                return Err(Error::FailedToCreateObject);
            }
            let side = (metrics.square_half * 2) as i32;

            lvgl::lv_obj_remove_style_all(center);
            lvgl::lv_obj_set_size(center, side, side);
            lvgl::lv_obj_set_align(center, lvgl::lv_align_t_LV_ALIGN_CENTER);
            lvgl::lv_obj_set_style_radius(
                center,
                metrics.square_radius as i32,
                lvgl::LV_STATE_DEFAULT,
            );
            match style {
                LogoStyle::Filled => {
                    lvgl::lv_obj_set_style_bg_opa(center, lvgl::LV_OPA_COVER as _, 0);
                }
                LogoStyle::Outline => {
                    lvgl::lv_obj_set_style_border_width(center, 1, lvgl::LV_STATE_DEFAULT);
                    lvgl::lv_obj_set_style_border_opa(center, lvgl::LV_OPA_COVER as _, 0);
                }
            }
            lvgl::lv_obj_remove_flag(center, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
            lvgl::lv_obj_add_flag(center, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
        }

        Ok(())
    }

    unsafe fn apply_color(object: *mut lvgl::lv_obj_t, color: Color) {
        let color = color.into_lvgl_color();

        unsafe {
            for index in 0..Self::ARM_COUNT {
                let arm = lvgl::lv_obj_get_child(object, index as i32);
                lvgl::lv_obj_set_style_image_recolor(arm, color, lvgl::LV_STATE_DEFAULT);
            }
            let center = lvgl::lv_obj_get_child(object, Self::CENTER_INDEX as i32);
            lvgl::lv_obj_set_style_bg_color(center, color, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_border_color(center, color, lvgl::LV_STATE_DEFAULT);
        }
    }

    /// Repaint the logo with a fixed color.
    ///
    /// # Safety
    /// The caller must hold the LVGL lock.
    pub unsafe fn set_color(&self, color: Color) {
        unsafe { Self::apply_color(self.object, color) }
    }

    /// Paint the logo with a theme palette color and follow later palette or
    /// dark/light changes.
    ///
    /// # Safety
    /// The caller must hold the LVGL lock.
    pub unsafe fn follow_palette_color(&self, color: theme::PaletteColor) {
        unsafe {
            if let Some(value) = theme::get_palette_color(self.object, color) {
                Self::apply_color(self.object, value);
            }
            lvgl::lv_obj_add_event_cb(
                self.object,
                Some(palette_color_changed),
                lvgl::lv_event_code_t_LV_EVENT_STYLE_CHANGED,
                color as usize as *mut core::ffi::c_void,
            );
        }
    }

    pub fn get_inner_object(&self) -> *mut lvgl::lv_obj_t {
        self.object
    }

    /// Arm object, in [`Self::ARM_COUNT`] order.
    ///
    /// # Safety
    /// The caller must hold the LVGL lock.
    pub unsafe fn get_arm(&self, index: usize) -> *mut lvgl::lv_obj_t {
        unsafe { lvgl::lv_obj_get_child(self.object, index.min(Self::ARM_COUNT - 1) as i32) }
    }

    /// Center square object.
    ///
    /// # Safety
    /// The caller must hold the LVGL lock.
    pub unsafe fn get_center(&self) -> *mut lvgl::lv_obj_t {
        unsafe { lvgl::lv_obj_get_child(self.object, Self::CENTER_INDEX as i32) }
    }
}

impl Drop for Logo {
    fn drop(&mut self) {
        unsafe {
            // Delete the images before releasing the bitmaps they reference.
            lvgl::lv_obj_delete(self.object);
            for buffer in self.buffers {
                if !buffer.is_null() {
                    lvgl::lv_image_cache_drop(buffer as *const _);
                    lvgl::lv_draw_buf_destroy(buffer);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn arm_bitmap(size: u16, style: LogoStyle) -> (Metrics, Vec<u8>) {
        let metrics = Metrics::new(size);
        let side = metrics.arm_size() as usize;
        let mut bitmap = vec![0; side * side];
        rasterize_arm(&metrics, &mut bitmap, side);
        if style == LogoStyle::Outline {
            outline(&mut bitmap, side, side);
        }
        (metrics, bitmap)
    }

    /// Shoelace area of the unhinted arm, in source units².
    fn arm_source_area() -> f32 {
        let mut points = vec![ARM_START];
        points.extend(ARM_PATH.map(|segment| match segment {
            Segment::Line(x, y) | Segment::Cubic(.., x, y) => (x, y),
        }));
        let mut area = 0.0;
        for index in 0..points.len() {
            let (x0, y0) = points[index];
            let (x1, y1) = points[(index + 1) % points.len()];
            area += x0 * y1 - x1 * y0;
        }
        (area / 2.0).abs()
    }

    #[test]
    fn metrics_keep_edges_snapped_and_separations_open() {
        for size in [16, 32, 64, 96, 128, 160, 192, 256, 320] {
            let metrics = Metrics::new(size);
            assert!(metrics.arm_inner >= 1, "{size}: arms must not touch");
            assert!(metrics.notch > metrics.square_half, "{size}: square gap");
            assert!(metrics.notch > metrics.arm_inner, "{size}: notch depth");
            assert!(metrics.square_half < size / 2);
        }
        // Integer factors used by the bootsplash and the desktop.
        assert_eq!(
            Metrics::new(160),
            Metrics {
                size: 160,
                arm_inner: 7,
                notch: 25,
                square_half: 15,
                square_radius: 1,
            }
        );
    }

    #[test]
    fn arm_bitmap_matches_supplied_area() {
        let source_area = arm_source_area();
        for size in [64, 128, 160, 320] {
            let (_, bitmap) = arm_bitmap(size, LogoStyle::Filled);
            let scale = size as f32 / EXTENT;
            let expected = source_area * scale * scale;
            let actual = bitmap
                .iter()
                .map(|&alpha| alpha as f32 / 255.0)
                .sum::<f32>();
            let error = (actual - expected).abs() / expected;
            // Corner flattening and pixel snapping change the area slightly.
            assert!(error < 0.04, "{size}: {actual} vs {expected}");
        }
    }

    #[test]
    fn arm_bitmap_has_crisp_snapped_edges() {
        let (metrics, bitmap) = arm_bitmap(160, LogoStyle::Filled);
        let side = metrics.arm_size() as usize;
        let notch_depth = (metrics.notch - metrics.arm_inner) as usize;

        // Outer right edge, between the rounded corner and the chamfer.
        assert_eq!(bitmap[(side / 4) * side + side - 1], 255);
        // Inner left edge, between the chamfer and the notch.
        let row = side - notch_depth - 2;
        assert_eq!(bitmap[row * side], 255);
        // Notch: its snapped top and right edges have no partial pixels.
        assert_eq!(bitmap[(side - notch_depth - 1) * side], 255);
        assert_eq!(bitmap[(side - notch_depth) * side], 0);
        assert_eq!(bitmap[(side - 1) * side + notch_depth - 1], 0);
        assert_eq!(bitmap[(side - 1) * side + notch_depth], 255);
        // Chamfered and rounded corners stay transparent.
        assert_eq!(bitmap[0], 0);
        assert_eq!(bitmap[side - 1], 0);
        assert_eq!(bitmap[side * side - 1], 0);
    }

    #[test]
    fn outline_keeps_only_the_contour() {
        let (metrics, filled) = arm_bitmap(128, LogoStyle::Filled);
        let (_, outlined) = arm_bitmap(128, LogoStyle::Outline);
        let side = metrics.arm_size() as usize;
        let middle = (side / 2) * side + side / 2;

        assert_eq!(filled[middle], 255);
        assert_eq!(outlined[middle], 0);
        assert_eq!(outlined[(side / 4) * side + side - 1], 255);
        let total = |bitmap: &[u8]| bitmap.iter().map(|&alpha| alpha as u32).sum::<u32>();
        assert!(total(&outlined) * 4 < total(&filled));
    }
}
