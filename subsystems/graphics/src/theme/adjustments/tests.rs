//! Renders fixed pointer scenarios on a 480x320 display with 20-row draw buffers
//! and counts what LVGL redraws: with the theme as it ships, with Xila's
//! invalidation fixes, and in light rendering mode.
//!
//! Run with `--nocapture` to print the table; use a release build to compare the
//! render times.

extern crate std;

use core::{ffi::c_void, ptr};
use portable_atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::{boxed::Box, ffi::CString, format, time::Instant, vec, vec::Vec};

use super::*;

const WIDTH: i32 = 480;
const HEIGHT: i32 = 320;
const SCREEN_PIXELS: u64 = (WIDTH * HEIGHT) as u64;
const DRAW_BUFFER_PIXELS: usize = WIDTH as usize * 20;
/// Period of the display refresh and of the pointer reads, in milliseconds.
const PERIOD: u32 = 10;

static TICKS: AtomicU32 = AtomicU32::new(0);
static POINTER_X: AtomicI32 = AtomicI32::new(0);
static POINTER_Y: AtomicI32 = AtomicI32::new(0);
static POINTER_PRESSED: AtomicBool = AtomicBool::new(false);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static FLUSHED_PIXELS: AtomicU64 = AtomicU64::new(0);
static RENDER_NANOSECONDS: AtomicU64 = AtomicU64::new(0);
/// A color to look for in what is flushed, and how many pixels had it.
static PROBE_COLOR: AtomicU32 = AtomicU32::new(u32::MAX);
static PROBE_PIXELS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn tick() -> u32 {
    TICKS.load(Ordering::Relaxed)
}

unsafe extern "C" fn read_pointer(_: *mut lvgl::lv_indev_t, data: *mut lvgl::lv_indev_data_t) {
    unsafe {
        (*data).point.x = POINTER_X.load(Ordering::Relaxed);
        (*data).point.y = POINTER_Y.load(Ordering::Relaxed);
        (*data).state = if POINTER_PRESSED.load(Ordering::Relaxed) {
            lvgl::lv_indev_state_t_LV_INDEV_STATE_PRESSED
        } else {
            lvgl::lv_indev_state_t_LV_INDEV_STATE_RELEASED
        };
    }
}

unsafe extern "C" fn flush(
    display: *mut lvgl::lv_display_t,
    area: *const lvgl::lv_area_t,
    pixels: *mut u8,
) {
    unsafe {
        let area = &*area;
        let count = ((area.x2 - area.x1 + 1) * (area.y2 - area.y1 + 1)) as usize;
        FLUSHED_PIXELS.fetch_add(count as u64, Ordering::Relaxed);
        let probe = PROBE_COLOR.load(Ordering::Relaxed);
        if probe != u32::MAX {
            let pixels = core::slice::from_raw_parts(pixels.cast::<u32>(), count);
            let matches = pixels
                .iter()
                .filter(|pixel| **pixel & 0x00ff_ffff == probe)
                .count();
            PROBE_PIXELS.fetch_add(matches as u64, Ordering::Relaxed);
        }
        lvgl::lv_display_flush_ready(display);
    }
}

unsafe extern "C" fn record(event: *mut lvgl::lv_event_t) {
    unsafe {
        let started = lvgl::lv_event_get_user_data(event).cast::<Option<Instant>>();
        if lvgl::lv_event_get_code(event) == lvgl::lv_event_code_t_LV_EVENT_RENDER_START {
            *started = Some(Instant::now());
        } else if let Some(started) = (*started).take() {
            FRAMES.fetch_add(1, Ordering::Relaxed);
            RENDER_NANOSECONDS.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }
}

/// How a scenario display is themed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Theme {
    /// The shadcn theme alone, as before this module existed.
    Shipped,
    Mode(RenderingMode),
}

#[derive(Clone, Copy, Debug, Default)]
struct Cost {
    frames: u32,
    pixels: u64,
    microseconds: u64,
}

fn reset() {
    FRAMES.store(0, Ordering::Relaxed);
    FLUSHED_PIXELS.store(0, Ordering::Relaxed);
    RENDER_NANOSECONDS.store(0, Ordering::Relaxed);
    PROBE_PIXELS.store(0, Ordering::Relaxed);
}

fn cost() -> Cost {
    Cost {
        frames: FRAMES.load(Ordering::Relaxed),
        pixels: FLUSHED_PIXELS.load(Ordering::Relaxed),
        microseconds: RENDER_NANOSECONDS.load(Ordering::Relaxed) / 1000,
    }
}

/// Lets `milliseconds` pass, running LVGL's timers every period.
unsafe fn wait(milliseconds: u32) {
    for _ in 0..milliseconds / PERIOD {
        TICKS.fetch_add(PERIOD, Ordering::Relaxed);
        unsafe { lvgl::lv_timer_handler() };
    }
}

unsafe fn press(x: i32, y: i32) {
    POINTER_X.store(x, Ordering::Relaxed);
    POINTER_Y.store(y, Ordering::Relaxed);
    POINTER_PRESSED.store(true, Ordering::Relaxed);
    unsafe { wait(PERIOD) };
}

unsafe fn release() {
    POINTER_PRESSED.store(false, Ordering::Relaxed);
    unsafe { wait(PERIOD) };
}

/// Presses, holds for 100 ms, releases and lets the interface settle.
unsafe fn tap(x: i32, y: i32) {
    unsafe {
        press(x, y);
        wait(100);
        release();
        wait(400);
    }
}

/// Drags from (`x`, `from`) to (`x`, `to`) in 10-pixel steps, then lets the
/// scroll settle.
unsafe fn drag(x: i32, from: i32, to: i32) {
    unsafe {
        press(x, from);
        let step = if to > from { 10 } else { -10 };
        let mut y = from;
        while y != to {
            y += step;
            press(x, y);
        }
        release();
        wait(1500);
    }
}

struct Scene {
    display: *mut lvgl::lv_display_t,
    pointer: *mut lvgl::lv_indev_t,
    screen: *mut lvgl::lv_obj_t,
    // Kept alive for LVGL, which holds pointers into them.
    _buffers: [Vec<u32>; 2],
    _render_started: Box<Option<Instant>>,
}

impl Scene {
    unsafe fn new(theme: Theme) -> Self {
        unsafe {
            TICKS.store(0, Ordering::Relaxed);
            POINTER_PRESSED.store(false, Ordering::Relaxed);
            PROBE_COLOR.store(u32::MAX, Ordering::Relaxed);
            lvgl::lv_init();
            lvgl::lv_tick_set_cb(Some(tick));

            let display = lvgl::lv_display_create(WIDTH, HEIGHT);
            let mut buffers = [
                vec![0u32; DRAW_BUFFER_PIXELS],
                vec![0u32; DRAW_BUFFER_PIXELS],
            ];
            lvgl::lv_display_set_buffers(
                display,
                buffers[0].as_mut_ptr().cast(),
                buffers[1].as_mut_ptr().cast(),
                (DRAW_BUFFER_PIXELS * size_of::<u32>()) as u32,
                lvgl::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
            );
            lvgl::lv_display_set_flush_cb(display, Some(flush));
            lvgl::lv_timer_set_period(lvgl::lv_display_get_refr_timer(display), PERIOD);

            match theme {
                Theme::Shipped => {
                    let fonts = lvgl::lv_theme_shadcn_fonts_t {
                        small: crate::fonts::get_font_small(),
                        normal: crate::fonts::get_font_medium(),
                        large: crate::fonts::get_font_large(),
                        mono: crate::fonts::get_font_monospace_medium(),
                        h1: ptr::null(),
                        h2: ptr::null(),
                        h3: ptr::null(),
                        medium: ptr::null(),
                    };
                    lvgl::lv_theme_shadcn_init(
                        display,
                        lvgl::lv_theme_shadcn_palette_zinc_dark(),
                        &fonts,
                        lvgl::lv_theme_shadcn_flags_t_LV_THEME_SHADCN_FLAG_NONE,
                    );
                }
                Theme::Mode(mode) => {
                    super::super::initialize_display(display).unwrap();
                    set_display_rendering_mode(display, mode).unwrap();
                }
            }

            let pointer = lvgl::lv_indev_create();
            lvgl::lv_indev_set_type(pointer, lvgl::lv_indev_type_t_LV_INDEV_TYPE_POINTER);
            lvgl::lv_indev_set_display(pointer, display);
            lvgl::lv_indev_set_read_cb(pointer, Some(read_pointer));
            lvgl::lv_timer_set_period(lvgl::lv_indev_get_read_timer(pointer), PERIOD);

            let mut render_started = Box::new(None);
            for code in [
                lvgl::lv_event_code_t_LV_EVENT_RENDER_START,
                lvgl::lv_event_code_t_LV_EVENT_RENDER_READY,
            ] {
                lvgl::lv_display_add_event_cb(
                    display,
                    Some(record),
                    code,
                    ptr::from_mut(&mut *render_started).cast::<c_void>(),
                );
            }

            Self {
                display,
                pointer,
                screen: lvgl::lv_display_get_screen_active(display),
                _buffers: buffers,
                _render_started: render_started,
            }
        }
    }

    /// A full-screen card, as an application window's body is.
    unsafe fn window(&self) -> *mut lvgl::lv_obj_t {
        unsafe {
            let window = lvgl::lv_obj_create(self.screen);
            lvgl::lv_obj_set_size(window, WIDTH, HEIGHT);
            window
        }
    }

    /// A full-screen list of 30 entries.
    unsafe fn list(&self) -> *mut lvgl::lv_obj_t {
        unsafe {
            let list = lvgl::lv_list_create(self.screen);
            lvgl::lv_obj_set_size(list, WIDTH, HEIGHT);
            for index in 0..30 {
                let text = CString::new(format!("Entry {index}")).unwrap();
                lvgl::lv_list_add_button(list, ptr::null(), text.as_ptr());
            }
            list
        }
    }

    /// Runs `scenario` once the scene has been drawn, and returns its cost.
    unsafe fn measure(&self, scenario: impl FnOnce()) -> Cost {
        unsafe { wait(100) };
        reset();
        scenario();
        cost()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        unsafe {
            lvgl::lv_indev_delete(self.pointer);
            lvgl::lv_display_delete(self.display);
            lvgl::lv_deinit();
        }
    }
}

/// Touching the empty part of a window.
unsafe fn touch_window(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        scene.window();
        scene.measure(|| tap(240, 200))
    }
}

/// Tapping a button in a window.
unsafe fn tap_button(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        let button = lvgl::lv_button_create(scene.window());
        lvgl::lv_obj_set_pos(button, 20, 20);
        lvgl::lv_obj_set_size(button, 120, 40);
        scene.measure(|| tap(60, 60))
    }
}

/// Typing five keys on the on-screen keyboard.
unsafe fn type_keys(theme: Theme) -> (Cost, u64) {
    unsafe {
        let scene = Scene::new(theme);
        let window = scene.window();
        let text_area = lvgl::lv_textarea_create(window);
        lvgl::lv_obj_set_width(text_area, 400);
        let keyboard = lvgl::lv_keyboard_create(lvgl::lv_display_get_layer_top(scene.display));
        lvgl::lv_obj_set_size(keyboard, WIDTH, 160);
        lvgl::lv_obj_align(keyboard, lvgl::lv_align_t_LV_ALIGN_BOTTOM_MID, 0, 0);
        lvgl::lv_keyboard_set_textarea(keyboard, text_area);

        let palette = &*lvgl::lv_theme_shadcn_get_palette(scene.display);
        let pressed = lvgl::lv_color_mix(palette.secondary_fg, palette.secondary, KEY_PRESS_MIX);
        wait(100);

        // The pressed key shows the theme's pressed color, and loses it again.
        PROBE_COLOR.store(
            lvgl::lv_color_to_u32(pressed) & 0x00ff_ffff,
            Ordering::Relaxed,
        );
        reset();
        press(120, 220);
        wait(100);
        let pressed_pixels = PROBE_PIXELS.load(Ordering::Relaxed);
        release();
        wait(400);
        reset();
        lvgl::lv_obj_invalidate(keyboard);
        wait(100);
        assert_eq!(PROBE_PIXELS.load(Ordering::Relaxed), 0, "{theme:?}");
        PROBE_COLOR.store(u32::MAX, Ordering::Relaxed);

        let cost = scene.measure(|| {
            for x in [120, 170, 220, 270, 320] {
                tap(x, 220);
            }
        });
        (cost, pressed_pixels)
    }
}

/// Dragging a list up by 150 pixels.
unsafe fn scroll_list(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        let list = scene.list();
        let cost = scene.measure(|| drag(240, 250, 100));
        // The drag and its momentum carry the list as far in every mode.
        assert_eq!(lvgl::lv_obj_get_scroll_y(list), 454);
        cost
    }
}

/// Dragging a list down past its top, and letting it return.
unsafe fn overscroll_list(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        scene.list();
        scene.measure(|| drag(240, 100, 250))
    }
}

/// Showing the second page of a full-screen tile view, as the shell's desk does.
unsafe fn switch_tile(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        let tiles = lvgl::lv_tileview_create(scene.screen);
        lvgl::lv_obj_set_size(tiles, WIDTH, HEIGHT);
        lvgl::lv_tileview_add_tile(tiles, 0, 0, lvgl::lv_dir_t_LV_DIR_BOTTOM);
        let second = lvgl::lv_tileview_add_tile(tiles, 0, 1, lvgl::lv_dir_t_LV_DIR_TOP);
        for index in 0..8 {
            let item = lvgl::lv_button_create(second);
            lvgl::lv_obj_set_pos(item, (index % 4) * 110 + 16, (index / 4) * 80 + 16);
            lvgl::lv_obj_set_size(item, 96, 64);
        }
        let cost = scene.measure(|| {
            lvgl::lv_tileview_set_tile_by_index(tiles, 0, 1, true);
            wait(1000);
        });
        assert_eq!(lvgl::lv_tileview_get_tile_active(tiles), second);
        assert_eq!(lvgl::lv_obj_get_scroll_y(tiles), HEIGHT);
        cost
    }
}

/// A hundred full redraws of a list.
unsafe fn redraw_list(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        scene.list();
        scene.measure(|| {
            for _ in 0..100 {
                lvgl::lv_obj_invalidate(scene.screen);
                lvgl::lv_refr_now(scene.display);
            }
        })
    }
}

/// A hundred full redraws of a window under a message box.
unsafe fn redraw_modal(theme: Theme) -> Cost {
    unsafe {
        let scene = Scene::new(theme);
        let window = scene.window();
        for index in 0..6 {
            let button = lvgl::lv_button_create(window);
            lvgl::lv_obj_set_pos(button, (index % 3) * 150, (index / 3) * 60);
            lvgl::lv_obj_set_size(button, 130, 40);
        }
        let message_box = lvgl::lv_msgbox_create(ptr::null_mut());
        lvgl::lv_msgbox_add_title(message_box, c"Delete".as_ptr());
        lvgl::lv_msgbox_add_text(message_box, c"Delete this file?".as_ptr());
        lvgl::lv_msgbox_add_footer_button(message_box, c"Cancel".as_ptr());
        scene.measure(|| {
            for _ in 0..100 {
                lvgl::lv_obj_invalidate(scene.screen);
                lvgl::lv_refr_now(scene.display);
            }
        })
    }
}

fn print(name: &str, costs: [Cost; 3]) {
    let cell = |cost: Cost| {
        format!(
            "{:>3} frames {:>9} px {:>7} us",
            cost.frames, cost.pixels, cost.microseconds
        )
    };
    std::eprintln!(
        "{name:<16} | shipped {} | default {} | light {}",
        cell(costs[0]),
        cell(costs[1]),
        cell(costs[2]),
    );
}

const THEMES: [Theme; 3] = [
    Theme::Shipped,
    Theme::Mode(RenderingMode::Default),
    Theme::Mode(RenderingMode::Light),
];

#[test]
fn adjustments_cut_what_pointer_scenarios_redraw() {
    unsafe {
        let window = THEMES.map(|theme| touch_window(theme));
        print("touch window", window);
        // The theme redraws a pressed window twice; nothing changes in it.
        assert_eq!(window[0].pixels, 2 * SCREEN_PIXELS);
        assert_eq!(window[1].pixels, 0);
        assert_eq!(window[2].pixels, 0);

        let button = THEMES.map(|theme| tap_button(theme));
        print("tap button", button);
        // Default mode keeps the theme's transitions; light mode shows the
        // pressed and the released button, and nothing in between.
        assert_eq!(button[1].frames, button[0].frames);
        assert_eq!(button[2].frames, 2);

        let keys = THEMES.map(|theme| type_keys(theme));
        print("type 5 keys", keys.map(|(cost, _)| cost));
        // Each press and release redraws the whole 480x160 keyboard as shipped,
        // and the key alone with the fix: what is left is mostly the text area
        // showing the new character. The key still shows as pressed.
        assert!(keys[0].0.pixels >= 10 * 480 * 160);
        assert!(keys[1].0.pixels * 5 < keys[0].0.pixels);
        assert!(keys[2].0.pixels * 5 < keys[0].0.pixels);
        for (_, pressed_pixels) in keys {
            assert!(pressed_pixels > 500, "{pressed_pixels}");
        }

        let scroll = THEMES.map(|theme| scroll_list(theme));
        print("scroll list", scroll);
        // In light mode the momentum takes half the frames, and the end of the
        // scroll does not redraw the list once more.
        assert_eq!(scroll[1].pixels, scroll[0].pixels);
        assert!(scroll[2].frames * 4 < scroll[1].frames * 3);

        let overscroll = THEMES.map(|theme| overscroll_list(theme));
        print("overscroll list", overscroll);
        assert!(overscroll[2].frames < overscroll[1].frames);

        let tile = THEMES.map(|theme| switch_tile(theme));
        print("switch tile", tile);
        assert!(tile[1].frames > 3);
        assert_eq!(tile[2].frames, 1);

        // Same pixels: these two only differ in render time.
        print("redraw list x100", THEMES.map(|theme| redraw_list(theme)));
        print("redraw modal x100", THEMES.map(|theme| redraw_modal(theme)));
    }
}

#[test]
fn light_mode_overrides_theme_styles_and_can_be_left() {
    unsafe {
        let scene = Scene::new(Theme::Mode(RenderingMode::Light));
        assert_eq!(get_rendering_mode(), RenderingMode::Light);
        let transition = |object| lvgl::lv_obj_get_style_transition(object, MAIN);
        let instant = INSTANT_TRANSITION.get().cast_const();

        let button = lvgl::lv_button_create(scene.screen);
        let list = lvgl::lv_list_create(scene.screen);
        assert_eq!(transition(button), instant);
        assert!(!lvgl::lv_obj_get_style_clip_corner(list, MAIN));
        // The theme's helpers still work: its own theme is still the display's.
        assert!(lvgl::lv_theme_shadcn_is_active(scene.display));
        lvgl::lv_theme_shadcn_set_variant(
            button,
            lvgl::lv_theme_shadcn_variant_t_LV_THEME_SHADCN_VARIANT_GHOST,
        );
        assert_eq!(
            lvgl::lv_obj_get_style_bg_opa(button, MAIN),
            lvgl::LV_OPA_TRANSP
        );

        // Leaving light mode gives existing and new objects the theme's look.
        set_display_rendering_mode(scene.display, RenderingMode::Default).unwrap();
        assert_eq!(get_rendering_mode(), RenderingMode::Default);
        assert_ne!(transition(button), instant);
        assert!(!transition(button).is_null());
        assert!(lvgl::lv_obj_get_style_clip_corner(list, MAIN));
        assert!(lvgl::lv_obj_get_style_clip_corner(
            lvgl::lv_list_create(scene.screen),
            MAIN
        ));

        // And existing objects enter it, including a window, whose class
        // derives from the plain object.
        let window = crate::Window::new(scene.screen).unwrap().as_ptr();
        let window = (*window).as_object_mutable();
        assert_ne!(transition(window), instant);
        set_display_rendering_mode(scene.display, RenderingMode::Light).unwrap();
        assert_eq!(transition(button), instant);
        assert_eq!(transition(window), instant);
        assert!(!lvgl::lv_obj_get_style_clip_corner(list, MAIN));
        set_display_rendering_mode(scene.display, RenderingMode::Default).unwrap();
    }
}

#[test]
fn leaving_light_mode_restores_the_theme_exactly() {
    unsafe {
        let scene = Scene::new(Theme::Mode(RenderingMode::Default));
        let scrolled = lvgl::lv_state_t_LV_STATE_SCROLLED as lvgl::lv_state_t;
        // The scrollbar's opacity while the object scrolls.
        let scrolled_opacity = |object| {
            lvgl::lv_obj_add_state(object, scrolled);
            let opacity = lvgl::lv_obj_get_style_bg_opa(object, SCROLLBAR);
            lvgl::lv_obj_remove_state(object, scrolled);
            opacity
        };
        let transition = |object| lvgl::lv_obj_get_style_transition(object, MAIN);

        let list = scene.list();
        let card = scene.window();
        // A class the theme does not style, derived from the plain object.
        let image = lvgl::lv_image_create(scene.screen);
        let themed = [list, card].map(scrolled_opacity);
        let shown = scrolled_opacity(scene.screen);

        for _ in 0..2 {
            set_display_rendering_mode(scene.display, RenderingMode::Light).unwrap();
            for object in [list, card] {
                assert_ne!(scrolled_opacity(object), themed[0]);
            }
            assert!(transition(image).is_null());
            set_display_rendering_mode(scene.display, RenderingMode::Default).unwrap();
            assert_eq!([list, card].map(scrolled_opacity), themed);
            assert_eq!(scrolled_opacity(scene.screen), shown);
            assert!(transition(image).is_null());
        }

        // Objects created in light mode get the theme's look as well.
        set_display_rendering_mode(scene.display, RenderingMode::Light).unwrap();
        let late = scene.list();
        assert_ne!(scrolled_opacity(late), themed[0]);
        set_display_rendering_mode(scene.display, RenderingMode::Default).unwrap();
        assert_eq!(scrolled_opacity(late), themed[0]);
    }
}
