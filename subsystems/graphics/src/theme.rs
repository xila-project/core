//! Xila's integration of the shared `lvgl_theme_shadcn` theme.
//!
//! The C theme owns shared styles and follows the display's lifetime. Call theme
//! helpers while holding the graphics manager's LVGL lock.

use crate::{Color, Display, Error, Result, ffi, fonts};

pub(crate) fn initialize(display: &Display) -> Result<()> {
    unsafe { initialize_display(display.get_lvgl_display()) }
}

unsafe fn initialize_display(display: *mut ffi::lv_display_t) -> Result<()> {
    let fonts = ffi::lv_theme_shadcn_fonts_t {
        small: fonts::get_font_small(),
        normal: fonts::get_font_medium(),
        large: fonts::get_font_large(),
        mono: fonts::get_font_monospace_medium(),
    };
    // The theme copies the palette/fonts and releases its state when the display
    // is deleted. No Rust-owned callback, parent theme or leaked Box is needed.
    let theme = unsafe {
        ffi::lv_theme_shadcn_init(
            display,
            ffi::lv_theme_shadcn_palette_zinc_dark(),
            &fonts,
            ffi::lv_theme_shadcn_flags_t_LV_THEME_SHADCN_FLAG_NONE,
        )
    };
    if theme.is_null() {
        return Err(Error::OutOfMemory);
    }
    Ok(())
}

pub(crate) fn update(
    display: &Display,
    primary_color: Color,
    secondary_color: Color,
    is_dark: bool,
) {
    unsafe {
        update_display(
            display.get_lvgl_display(),
            primary_color,
            secondary_color,
            is_dark,
        )
    }
}

unsafe fn update_display(
    display: *mut ffi::lv_display_t,
    primary_color: Color,
    secondary_color: Color,
    is_dark: bool,
) {
    unsafe {
        let mut palette = if is_dark {
            *ffi::lv_theme_shadcn_palette_zinc_dark()
        } else {
            *ffi::lv_theme_shadcn_palette_zinc_light()
        };
        palette.primary = primary_color.into_lvgl_color();
        palette.primary_fg = contrasting_foreground(primary_color).into_lvgl_color();
        palette.ring = palette.primary;
        palette.secondary = secondary_color.into_lvgl_color();
        palette.secondary_fg = contrasting_foreground(secondary_color).into_lvgl_color();
        ffi::lv_theme_shadcn_set_palette(display, &palette);
    }
}

fn contrasting_foreground(color: Color) -> Color {
    let brightness = 299 * color.get_red() as u32
        + 587 * color.get_green() as u32
        + 114 * color.get_blue() as u32;
    if brightness >= 128_000 {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

/// A semantic container surface that follows palette changes.
#[derive(Clone, Copy)]
pub enum Surface {
    Background,
    Card,
    Popover,
    Muted,
}

/// Apply a shared container surface instead of a local background color.
///
/// # Safety
/// `object` must be live and the caller must hold the LVGL lock.
pub unsafe fn set_surface(object: *mut ffi::lv_obj_t, surface: Surface) {
    let surface = match surface {
        Surface::Background => ffi::lv_theme_shadcn_surface_t_LV_THEME_SHADCN_SURFACE_BACKGROUND,
        Surface::Card => ffi::lv_theme_shadcn_surface_t_LV_THEME_SHADCN_SURFACE_CARD,
        Surface::Popover => ffi::lv_theme_shadcn_surface_t_LV_THEME_SHADCN_SURFACE_POPOVER,
        Surface::Muted => ffi::lv_theme_shadcn_surface_t_LV_THEME_SHADCN_SURFACE_MUTED,
    };
    unsafe { ffi::lv_theme_shadcn_set_surface(object, surface) };
}

/// A palette role, for drawings that LVGL styles cannot color through the theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum PaletteColor {
    Foreground,
    MutedForeground,
    Primary,
    Border,
    Input,
}

impl PaletteColor {
    const ALL: [Self; 5] = [
        Self::Foreground,
        Self::MutedForeground,
        Self::Primary,
        Self::Border,
        Self::Input,
    ];

    /// Inverse of `color as usize`, used to carry a role through LVGL user data.
    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Foreground)
    }
}

/// Current palette color of the display showing `object`, or `None` when the
/// theme is not active there.
///
/// # Safety
/// `object` must be live and the caller must hold the LVGL lock.
pub unsafe fn get_palette_color(object: *mut ffi::lv_obj_t, color: PaletteColor) -> Option<Color> {
    unsafe {
        let palette = ffi::lv_theme_shadcn_get_palette(ffi::lv_obj_get_display(object));
        let palette = palette.as_ref()?;
        let value = match color {
            PaletteColor::Foreground => palette.foreground,
            PaletteColor::MutedForeground => palette.muted_fg,
            PaletteColor::Primary => palette.primary,
            PaletteColor::Border => palette.border,
            PaletteColor::Input => palette.input,
        };
        Some(Color::from_lvgl_color(value))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    extern crate abi_definitions;
    use super::*;

    testing::instantiate_global_allocator!();

    #[test]
    fn palette_updates_preserve_widget_styles_and_logo_geometry() {
        unsafe {
            ffi::lv_init();
            let display = ffi::lv_display_create(320, 240);
            initialize_display(display).unwrap();
            let screen = ffi::lv_display_get_screen_active(display);
            let logos = [1, 2, 4].map(|factor| {
                (
                    crate::Logo::new(screen, factor, Color::WHITE).unwrap(),
                    factor,
                )
            });
            for (logo, factor) in &logos {
                assert_logo_geometry(logo, *factor);
            }
            // The desktop watermark follows the palette instead of a fixed color.
            let themed = &logos[2].0;
            themed.follow_palette_color(PaletteColor::Input);
            let arm_color = |logo: &crate::Logo| {
                ffi::lv_obj_get_style_image_recolor(logo.get_arm(0), ffi::LV_PART_MAIN)
            };
            assert!(ffi::lv_color_eq(
                arm_color(themed),
                (*ffi::lv_theme_shadcn_get_palette(display)).input
            ));
            let button = ffi::lv_button_create(screen);
            let surface = ffi::lv_obj_create(screen);
            set_surface(surface, Surface::Muted);
            let dark = *ffi::lv_theme_shadcn_get_palette(display);
            assert!(dark.dark);
            assert!(ffi::lv_color_eq(
                ffi::lv_obj_get_style_bg_color(button, 0),
                dark.primary
            ));
            assert!(ffi::lv_color_eq(
                ffi::lv_obj_get_style_bg_color(surface, 0),
                dark.muted
            ));

            let primary = Color::new(0x25, 0x63, 0xeb);
            let secondary = Color::new(0xf4, 0xf4, 0xf5);
            update_display(display, primary, secondary, false);
            let light = *ffi::lv_theme_shadcn_get_palette(display);
            assert!(!light.dark);
            assert!(ffi::lv_color_eq(light.primary, primary.into_lvgl_color()));
            assert!(ffi::lv_color_eq(
                light.secondary,
                secondary.into_lvgl_color()
            ));
            assert!(ffi::lv_color_eq(
                light.primary_fg,
                Color::WHITE.into_lvgl_color()
            ));
            assert!(ffi::lv_color_eq(
                light.secondary_fg,
                Color::BLACK.into_lvgl_color()
            ));
            assert!(ffi::lv_color_eq(
                ffi::lv_obj_get_style_bg_color(button, 0),
                light.primary
            ));
            assert!(ffi::lv_color_eq(
                ffi::lv_obj_get_style_bg_color(surface, 0),
                light.muted
            ));

            assert!(ffi::lv_color_eq(arm_color(&logos[2].0), light.input));
            assert!(!ffi::lv_color_eq(arm_color(&logos[2].0), dark.input));
            // Fixed-color logos keep their color.
            assert!(ffi::lv_color_eq(
                arm_color(&logos[0].0),
                Color::WHITE.into_lvgl_color()
            ));
            for (logo, factor) in &logos {
                assert_logo_geometry(logo, *factor);
            }
            drop(logos);
            ffi::lv_display_delete(display);
            // Theme state must be gone before display storage can be reused.
            assert!(!ffi::lv_theme_shadcn_is_active(display));
            ffi::lv_deinit();
        }
    }
    unsafe fn assert_logo_geometry(logo: &crate::Logo, factor: u8) {
        unsafe {
            let object = logo.get_inner_object();
            let size = 32 * i32::from(factor);
            ffi::lv_obj_update_layout(object);
            assert_eq!(ffi::lv_obj_get_width(object), size);
            assert_eq!(ffi::lv_obj_get_height(object), size);
            assert_eq!(
                ffi::lv_obj_get_child_count(object),
                crate::Logo::ARM_COUNT as u32 + 1
            );
            let mut bounds = ffi::lv_area_t::default();
            ffi::lv_obj_get_coords(object, &mut bounds);
            let mut arm_side = None;
            for index in 0..crate::Logo::ARM_COUNT {
                let arm = logo.get_arm(index);
                let side = ffi::lv_obj_get_width(arm);
                assert_eq!(ffi::lv_obj_get_height(arm), side);
                assert_eq!(*arm_side.get_or_insert(side), side);
                // Arms are separated by a transparent cross through the center.
                assert!(side < size / 2);
                let mut coordinates = ffi::lv_area_t::default();
                ffi::lv_obj_get_coords(arm, &mut coordinates);
                let expected_x = if index < 2 {
                    bounds.x1
                } else {
                    bounds.x2 - side + 1
                };
                let expected_y = if index == 0 || index == 3 {
                    bounds.y1
                } else {
                    bounds.y2 - side + 1
                };
                assert_eq!((coordinates.x1, coordinates.y1), (expected_x, expected_y));
            }
            let center = logo.get_center();
            let mut coordinates = ffi::lv_area_t::default();
            ffi::lv_obj_get_coords(center, &mut coordinates);
            assert_eq!(
                coordinates.x1 + coordinates.x2 + 1,
                bounds.x1 + bounds.x2 + 1,
                "center square must be centered"
            );
        }
    }
}
