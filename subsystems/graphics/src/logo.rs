//! Xila Monogram symbol drawn with LVGL.
//!
//! Geometry source: `xila-project/brand` `source/symbol.svg` at commit
//! `00d6686` (identity 2.0.0, designed by Martin Legand). The symbol is four
//! rounded diagonal arms around a separate rounded center square.
//!
//! A single LVGL object draws the symbol directly into the current layer.
//! Whole-pixel inner edges, notches and center-square edges preserve the gaps
//! between the pieces at every factor.

use core::{mem::ManuallyDrop, ptr};
use synchronization::once_lock::OnceLock;

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

    /// Side of one arm bounding square.
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

    /// Map a top-right arm point in source units to its local coordinates.
    fn map_arm_point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let arm_size = self.arm_size() as f32;
        let x = self.map_distance(x - CENTER) - self.arm_inner as f32;
        let y = (self.size / 2) as f32 - self.map_distance(CENTER - y);

        (x.clamp(0.0, arm_size), y.clamp(0.0, arm_size))
    }
}

/// The top-right arm's two edges, ordered from its outer horizontal edge
/// down to its inner horizontal edge. The obtuse corners and recessed notch
/// use two chords through each source Bézier's midpoint. The outside right
/// angle uses an LVGL rounded rectangle.
const LEFT_EDGE: [(f32, f32); 10] = [
    (485.12, 0.02),
    (475.51, 1.93),
    (467.36, 7.38),
    (344.69, 130.05),
    (339.24, 138.20),
    (337.33, 147.81),
    (337.33, 213.82),
    (374.18, 213.82),
    (394.98, 222.47),
    (403.63, 243.27),
];
const RIGHT_EDGE: [(f32, f32); 7] = [
    (617.45, 25.14),
    (617.45, 132.31),
    (615.54, 141.92),
    (610.09, 150.07),
    (487.40, 272.76),
    (479.25, 278.21),
    (469.64, 280.12),
];

/// Coordinates on pixel boundaries in the canonical (top-right) arm.
#[derive(Clone, Copy)]
struct Vertex {
    x: i32,
    y: i32,
}

struct Arm {
    x: i32,
    y: i32,
    mirror_x: bool,
    mirror_y: bool,
    side: i32,
}

impl Arm {
    fn vertex(&self, point: Vertex) -> lvgl::lv_point_precise_t {
        lvgl::lv_point_precise_t {
            x: self.x
                + if self.mirror_x {
                    self.side - point.x
                } else {
                    point.x
                },
            y: self.y
                + if self.mirror_y {
                    self.side - point.y
                } else {
                    point.y
                },
        }
    }

    /// Convert a half-open rectangle on pixel boundaries to LVGL's inclusive area.
    fn area(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> lvgl::lv_area_t {
        let a = self.vertex(Vertex { x: x1, y: y1 });
        let b = self.vertex(Vertex { x: x2, y: y2 });
        lvgl::lv_area_t {
            x1: a.x.min(b.x),
            x2: a.x.max(b.x) - 1,
            y1: a.y.min(b.y),
            y2: a.y.max(b.y) - 1,
        }
    }
}

struct Painter {
    layer: *mut lvgl::lv_layer_t,
    fill: lvgl::lv_draw_fill_dsc_t,
    triangle: lvgl::lv_draw_triangle_dsc_t,
    line: lvgl::lv_draw_line_dsc_t,
}

impl Painter {
    /// Paint one edge up to the arm's interior split column. The solid
    /// rectangle and the sloping triangle own disjoint columns, so their
    /// shared vertical edge never receives two antialiased fills.
    unsafe fn fill_edge(&mut self, arm: &Arm, edge: &[Vertex], left: bool, radius: i32) {
        let side = arm.side;
        let middle = side / 2;
        // The midline moves left between the upper and lower halves so
        // it stays inside the diagonal band at the top and bottom.
        for (top, bottom, split) in [(0, middle, side * 2 / 3), (middle, side, side / 3)] {
            let (near, far) = if left { (0, split) } else { (split, side) };
            let original = unsafe { (*self.layer)._clip_area };
            if !left && top == 0 && radius > 0 {
                let area = arm.area(split, 0, side - radius, radius);
                unsafe { lvgl::lv_draw_fill(self.layer, &self.fill, &area) };
            }
            for pair in edge.windows(2) {
                let [a, b] = [pair[0], pair[1]];
                let y1 = a.y.max(top);
                let y2 = b.y.min(bottom);
                if y1 >= y2 {
                    continue;
                }
                let (tip, base) = if (a.x < b.x) == left { (b, a) } else { (a, b) };
                let (rect_left, rect_right) = if left { (tip.x, far) } else { (near, tip.x) };
                if rect_left < rect_right {
                    let area = arm.area(rect_left, y1, rect_right, y2);
                    unsafe { lvgl::lv_draw_fill(self.layer, &self.fill, &area) };
                }

                let x1 = near.max(a.x.min(b.x));
                let x2 = far.min(a.x.max(b.x));
                if x1 < x2 {
                    let area = arm.area(x1, y1, x2, y2);
                    let mut clip = lvgl::lv_area_t::default();
                    if unsafe { lvgl::lv_area_intersect(&mut clip, &original, &area) } {
                        self.triangle.p = [
                            arm.vertex(tip),
                            arm.vertex(base),
                            arm.vertex(Vertex {
                                x: tip.x,
                                y: base.y,
                            }),
                        ];
                        unsafe {
                            (*self.layer)._clip_area = clip;
                            lvgl::lv_draw_triangle(self.layer, &self.triangle);
                            (*self.layer)._clip_area = original;
                        }
                    }
                }
            }
        }
    }

    /// Draw only the quadrant in the outside corner. The other arms mirror
    /// its pixel bounds in the same way as the straight edges.
    unsafe fn rounded_corner(&self, arm: &Arm, radius: i32, outline: bool) {
        let area = arm.area(arm.side - 2 * radius, 0, arm.side, 2 * radius);
        let quadrant = arm.area(arm.side - radius, 0, arm.side, radius);
        unsafe {
            let original = (*self.layer)._clip_area;
            let mut clip = lvgl::lv_area_t::default();
            if lvgl::lv_area_intersect(&mut clip, &original, &quadrant) {
                (*self.layer)._clip_area = clip;
                draw_rounded_rect(
                    self.layer,
                    &area,
                    self.fill.color,
                    radius,
                    outline,
                    self.fill.opa,
                );
                (*self.layer)._clip_area = original;
            }
        }
    }

    /// Stroke the inside of an edge. The row clip makes adjacent chords meet
    /// without accumulating opacity at their joins.
    unsafe fn stroke_edge(&mut self, arm: &Arm, edge: &[Vertex], left: bool) {
        let original = unsafe { (*self.layer)._clip_area };
        for pair in edge.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if a.y == b.y {
                if a.x != b.x {
                    // The notch and flattened corner chords can be horizontal.
                    let interior_below = (b.x < a.x) == left;
                    let y = (a.y - i32::from(!interior_below)).clamp(0, arm.side - 1);
                    let area = arm.area(a.x.min(b.x), y, a.x.max(b.x), y + 1);
                    unsafe { lvgl::lv_draw_fill(self.layer, &self.fill, &area) };
                }
                continue;
            }
            let mut p = arm.vertex(a);
            let mut q = arm.vertex(b);
            if p.y > q.y {
                core::mem::swap(&mut p, &mut q);
            }
            let interior_right = left != arm.mirror_x;
            let dx = q.x - p.x;
            // LVGL places a one-pixel steep stroke to the right of its
            // geometric line. For a shallow stroke the bias is along y.
            if dx.abs() <= q.y - p.y {
                let inset = i32::from(!interior_right);
                p.x -= inset;
                q.x -= inset;
            } else if interior_right == (dx > 0) {
                p.y -= 1;
                q.y -= 1;
            }
            let area = arm.area(0, a.y, arm.side, b.y);
            let mut clip = lvgl::lv_area_t::default();
            if unsafe { lvgl::lv_area_intersect(&mut clip, &original, &area) } {
                self.line.p1 = p;
                self.line.p2 = q;
                unsafe {
                    (*self.layer)._clip_area = clip;
                    lvgl::lv_draw_line(self.layer, &self.line);
                    (*self.layer)._clip_area = original;
                }
            }
        }
    }
}

unsafe fn draw_logo(object: *mut lvgl::lv_obj_t, layer: *mut lvgl::lv_layer_t) {
    unsafe {
        let instance = object.cast::<LogoObject>();
        let outline = (*instance).style == LogoStyle::Outline;
        let pose = (*instance).pose;
        let mut bounds = lvgl::lv_area_t::default();
        lvgl::lv_obj_get_coords(object, &mut bounds);
        let width = bounds.x2 - bounds.x1 + 1;
        let height = bounds.y2 - bounds.y1 + 1;
        let size = width.min(height).min(i32::from(u8::MAX) * 32) & !1;
        if size < 32 {
            return;
        }
        let x = bounds.x1 + (width - size) / 2;
        let y = bounds.y1 + (height - size) / 2;
        let metrics = Metrics::new(size as u16);
        let side = i32::from(metrics.arm_size());
        let map = |point| {
            let (x, y) = metrics.map_arm_point(point);
            Vertex {
                x: (x + 0.5) as i32,
                y: (y + 0.5) as i32,
            }
        };
        let left = LEFT_EDGE.map(map);
        let right = RIGHT_EDGE.map(map);
        let radius = right[0].y;
        let bottom_left = Vertex {
            x: i32::from(metrics.notch - metrics.arm_inner),
            y: side,
        };

        let color = lvgl::lv_obj_get_style_text_color(object, lvgl::LV_PART_MAIN);
        let mut painter = Painter {
            layer,
            fill: lvgl::lv_draw_fill_dsc_t::default(),
            triangle: lvgl::lv_draw_triangle_dsc_t::default(),
            line: lvgl::lv_draw_line_dsc_t::default(),
        };
        lvgl::lv_draw_fill_dsc_init(&mut painter.fill);
        lvgl::lv_draw_triangle_dsc_init(&mut painter.triangle);
        lvgl::lv_draw_line_dsc_init(&mut painter.line);
        painter.fill.color = color;
        painter.triangle.color = color;
        painter.line.color = color;
        painter.line.width = 1;
        painter.line.set_raw_end(1);

        let mut left_edge = [bottom_left; LEFT_EDGE.len() + 1];
        left_edge[..left.len()].copy_from_slice(&left);
        let mirrors = [(true, false), (true, true), (false, true), (false, false)];
        for ((mirror_x, mirror_y), arm_pose) in mirrors.into_iter().zip(pose.arms) {
            if arm_pose.opacity == 0 {
                continue;
            }
            let offset = i32::from(arm_pose.offset);
            let arm = Arm {
                x: if mirror_x {
                    x - offset
                } else {
                    x + size - side + offset
                },
                y: if mirror_y {
                    y + size - side + offset
                } else {
                    y - offset
                },
                mirror_x,
                mirror_y,
                side,
            };
            painter.fill.opa = arm_pose.opacity;
            painter.line.opa = arm_pose.opacity;
            painter.triangle.opa = arm_pose.opacity;
            painter.rounded_corner(&arm, radius, outline);
            if outline {
                let top_right = Vertex {
                    x: side - radius,
                    y: 0,
                };
                painter.stroke_edge(&arm, &left_edge, true);
                painter.stroke_edge(&arm, &right, false);
                painter.stroke_edge(&arm, &[left[0], top_right], false);
                painter.stroke_edge(&arm, &[bottom_left, right[right.len() - 1]], false);
            } else {
                painter.fill_edge(&arm, &left_edge, true, 0);
                painter.fill_edge(&arm, &right, false, radius);
            }
        }

        let half = i32::from(metrics.square_half);
        let center_x = x + size / 2;
        let center_y = y + size / 2;
        draw_rounded_rect(
            layer,
            &lvgl::lv_area_t {
                x1: center_x - half,
                y1: center_y - half,
                x2: center_x + half - 1,
                y2: center_y + half - 1,
            },
            color,
            i32::from(metrics.square_radius),
            outline,
            pose.center_opacity,
        );
    }
}

unsafe fn draw_rounded_rect(
    layer: *mut lvgl::lv_layer_t,
    area: &lvgl::lv_area_t,
    color: lvgl::lv_color_t,
    radius: i32,
    outline: bool,
    opacity: lvgl::lv_opa_t,
) {
    if opacity == 0 {
        return;
    }
    unsafe {
        let mut rect = lvgl::lv_draw_rect_dsc_t::default();
        lvgl::lv_draw_rect_dsc_init(&mut rect);
        rect.radius = radius;
        if outline {
            rect.bg_opa = 0;
            rect.border_width = 1;
            rect.border_color = color;
            rect.border_opa = opacity;
        } else {
            rect.bg_color = color;
            rect.bg_opa = opacity;
        }
        lvgl::lv_draw_rect(layer, &rect, area);
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

/// Visual state of one arm in an animated logo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogoArmPose {
    /// Outward displacement in pixels on each axis.
    pub offset: u16,
    /// Opacity from 0 (transparent) to 255 (opaque).
    pub opacity: u8,
}

/// Per-piece animation state, stored inline in the logo object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogoPose {
    /// Top-left, bottom-left, bottom-right, top-right.
    pub arms: [LogoArmPose; Logo::ARM_COUNT],
    /// Opacity from 0 (transparent) to 255 (opaque).
    pub center_opacity: u8,
}

impl LogoPose {
    /// Centered, fully opaque logo.
    pub const REST: Self = Self {
        arms: [LogoArmPose {
            offset: 0,
            opacity: 255,
        }; Logo::ARM_COUNT],
        center_opacity: 255,
    };

    fn outset(&self) -> i32 {
        i32::from(self.arms.iter().map(|arm| arm.offset).max().unwrap_or(0))
    }
}

#[repr(C)]
struct LogoObject {
    base: lvgl::lv_obj_t,
    pose: LogoPose,
    style: LogoStyle,
}

struct LogoClass(lvgl::lv_obj_class_t);

// The descriptor is immutable after initialization and points to static data.
unsafe impl Send for LogoClass {}
unsafe impl Sync for LogoClass {}

static LOGO_CLASS: OnceLock<LogoClass> = OnceLock::new();

fn logo_class() -> &'static lvgl::lv_obj_class_t {
    &LOGO_CLASS
        .get_or_init(|| {
            let mut class = lvgl::lv_obj_class_t {
                base_class: &raw const lvgl::lv_obj_class,
                constructor_cb: Some(construct_logo),
                event_cb: Some(logo_event),
                name: c"logo".as_ptr(),
                ..Default::default()
            };
            class.set_instance_size(size_of::<LogoObject>() as _);
            LogoClass(class)
        })
        .0
}

unsafe extern "C" fn construct_logo(_: *const lvgl::lv_obj_class_t, object: *mut lvgl::lv_obj_t) {
    unsafe {
        let instance = object.cast::<LogoObject>();
        ptr::addr_of_mut!((*instance).pose).write(LogoPose::REST);
        ptr::addr_of_mut!((*instance).style).write(LogoStyle::Filled);
    }
}

unsafe extern "C" fn logo_event(class: *const lvgl::lv_obj_class_t, event: *mut lvgl::lv_event_t) {
    unsafe {
        if lvgl::lv_obj_event_base(class, event) != lvgl::lv_result_t_LV_RESULT_OK {
            return;
        }
        let object = lvgl::lv_event_get_current_target(event).cast::<lvgl::lv_obj_t>();
        match lvgl::lv_event_get_code(event) {
            lvgl::lv_event_code_t_LV_EVENT_DRAW_MAIN => {
                draw_logo(object, lvgl::lv_event_get_layer(event));
            }
            lvgl::lv_event_code_t_LV_EVENT_REFR_EXT_DRAW_SIZE => {
                let pose = (*object.cast::<LogoObject>()).pose;
                lvgl::lv_event_set_ext_draw_size(event, pose.outset());
            }
            _ => {}
        }
    }
}

/// A logo that deletes its LVGL object on drop.
#[non_exhaustive]
pub struct Logo {
    pub object: *mut lvgl::lv_obj_t,
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
    pub const ARM_COUNT: usize = 4;

    pub fn get_factor(size: Point) -> u8 {
        let factor_x = size.get_x() / Self::BASE_SIZE.get_x();
        let factor_y = size.get_y() / Self::BASE_SIZE.get_y();

        factor_x.min(factor_y) as u8
    }

    /// Create a filled logo of `32 * factor` pixels.
    ///
    /// # Safety
    /// `parent` must be a live LVGL object and the caller must hold the LVGL lock.
    /// Keep the logo's object alive until the wrapper is dropped or converted
    /// with [`Self::into_inner_object`]. Drop the wrapper under the LVGL lock.
    pub unsafe fn new(parent: *mut lvgl::lv_obj_t, factor: u8, color: Color) -> Result<Self> {
        unsafe { Self::new_with_style(parent, factor, color, LogoStyle::Filled) }
    }

    /// Create a logo of `32 * factor` pixels with the given style.
    ///
    /// # Safety
    /// `parent` must be a live LVGL object and the caller must hold the LVGL lock.
    /// Keep the logo's object alive until the wrapper is dropped or converted
    /// with [`Self::into_inner_object`]. Drop the wrapper under the LVGL lock.
    pub unsafe fn new_with_style(
        parent: *mut lvgl::lv_obj_t,
        factor: u8,
        color: Color,
        style: LogoStyle,
    ) -> Result<Self> {
        if factor == 0 {
            return Err(Error::InvalidParameter);
        }
        let size = i32::from(Self::BASE_SIZE.get_x()) * i32::from(factor);

        unsafe {
            let object = lvgl::lv_obj_class_create_obj(logo_class(), parent);

            if object.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_class_init_obj(object);
            (*object.cast::<LogoObject>()).style = style;

            // Decorative and theme independent: no surface, padding or input.
            lvgl::lv_obj_remove_style_all(object);
            lvgl::lv_obj_set_size(object, size, size);
            lvgl::lv_obj_remove_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
            lvgl::lv_obj_remove_flag(object, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE);
            lvgl::lv_obj_set_style_text_color(object, color.into_lvgl_color(), 0);
            Ok(Self { object })
        }
    }

    /// Replace the animation pose of a logo, invalidating its old and new bounds.
    ///
    /// # Safety
    /// `object` must be a live object created by [`Self::new`] or
    /// [`Self::new_with_style`]. The caller must hold the LVGL lock.
    pub unsafe fn set_pose(object: *mut lvgl::lv_obj_t, pose: LogoPose) {
        unsafe {
            let instance = object.cast::<LogoObject>();
            let previous = (*instance).pose;
            if pose == previous {
                return;
            }
            lvgl::lv_obj_invalidate(object);
            (*instance).pose = pose;
            if pose.outset() != previous.outset() {
                lvgl::lv_obj_refresh_ext_draw_size(object);
            }
            lvgl::lv_obj_invalidate(object);
        }
    }

    unsafe fn apply_color(object: *mut lvgl::lv_obj_t, color: Color) {
        let color = color.into_lvgl_color();
        unsafe {
            // Setting a style sends STYLE_CHANGED to the logo itself.
            if !lvgl::lv_color_eq(lvgl::lv_obj_get_style_text_color(object, 0), color) {
                lvgl::lv_obj_set_style_text_color(object, color, 0);
            }
        }
    }

    /// Repaint the logo with a fixed color and stop following the palette.
    ///
    /// # Safety
    /// The caller must hold the LVGL lock.
    pub unsafe fn set_color(&self, color: Color) {
        unsafe {
            lvgl::lv_obj_remove_event_cb(self.object, Some(palette_color_changed));
            Self::apply_color(self.object, color);
        }
    }

    /// Paint the logo with a theme palette color and follow later palette or
    /// dark/light changes.
    ///
    /// # Safety
    /// The caller must hold the LVGL lock.
    pub unsafe fn follow_palette_color(&self, color: theme::PaletteColor) {
        unsafe {
            lvgl::lv_obj_remove_event_cb(self.object, Some(palette_color_changed));
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

    /// Leave the logo owned by the LVGL object tree.
    /// The returned pointer is valid until LVGL deletes the logo.
    pub fn into_inner_object(self) -> *mut lvgl::lv_obj_t {
        ManuallyDrop::new(self).object
    }
}

impl Drop for Logo {
    fn drop(&mut self) {
        unsafe { lvgl::lv_obj_delete(self.object) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_keep_edges_snapped_and_separations_open() {
        for size in (1..=u16::from(u8::MAX)).map(|factor| factor * 32) {
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
}
