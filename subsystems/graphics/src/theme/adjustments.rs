//! What Xila changes in the shadcn theme: invalidation fixes for every display,
//! and the light rendering mode.
//!
//! The theme exposes its palette, radius scale and a flag that only stops the
//! skeleton pulse, so the rest is done here: a callback chained after the theme's
//! own removes or overrides a few of its styles on each new object. The override
//! styles are constant and shared, so an object pays one style slot for each and
//! nothing has to be released.
//!
//! The theme has no shadows or gradients to remove, and its remaining blending
//! is cheap: a disabled control scales the opacity of what it draws, without an
//! intermediate layer, and a scrollbar is a few pixels wide.

use core::{cell::UnsafeCell, ffi::c_void, ptr};

use portable_atomic::{AtomicBool, Ordering};

use crate::{Error, Result, lvgl};

/// How much the theme may spend on rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderingMode {
    /// The full theme: animated state changes and scrolling, dimmed modal
    /// backdrops, clipped rounded corners.
    #[default]
    Default,
    /// For slow display buses and processors. State changes and the scrolls
    /// that LVGL animates take one frame, the momentum of a scroll takes half
    /// as many, modal backdrops are not blended, lists and tables do not clip
    /// their children to their rounded corners, and ending a scroll does not
    /// redraw the scrolled object.
    Light,
}

/// Light rendering is on. Read and written under the LVGL lock.
static LIGHT: AtomicBool = AtomicBool::new(false);

/// Static data that LVGL reads, and that only changes under the LVGL lock.
#[repr(transparent)]
struct Shared<T>(UnsafeCell<T>);

// SAFETY: every access happens under the LVGL lock.
unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    const fn get(&self) -> *mut T {
        self.0.get()
    }
}

const fn number(property: u32, value: i32) -> lvgl::lv_style_const_prop_t {
    lvgl::lv_style_const_prop_t {
        prop: property as lvgl::lv_style_prop_t,
        value: lvgl::lv_style_value_t { num: value },
    }
}

/// Ends a constant style's property list.
const END: lvgl::lv_style_const_prop_t = number(lvgl::_lv_style_id_t_LV_STYLE_PROP_INV, 0);

/// A style in LVGL's constant layout (`LV_STYLE_CONST_INIT`): its properties are
/// a static list instead of memory from LVGL's heap.
const fn constant_style(properties: *mut lvgl::lv_style_const_prop_t) -> lvgl::lv_style_t {
    lvgl::lv_style_t {
        values_and_props: properties.cast(),
        has_group: u32::MAX,
        prop_cnt: 255,
    }
}

/// The properties the theme animates on a state change.
static TRANSITION_PROPERTIES: [lvgl::lv_style_prop_t; 4] = [
    lvgl::_lv_style_id_t_LV_STYLE_BG_COLOR as lvgl::lv_style_prop_t,
    lvgl::_lv_style_id_t_LV_STYLE_BORDER_COLOR as lvgl::lv_style_prop_t,
    lvgl::_lv_style_id_t_LV_STYLE_OPA as lvgl::lv_style_prop_t,
    lvgl::_lv_style_id_t_LV_STYLE_PROP_INV as lvgl::lv_style_prop_t,
];

static INSTANT_TRANSITION: Shared<lvgl::lv_style_transition_dsc_t> =
    Shared::new(lvgl::lv_style_transition_dsc_t {
        props: TRANSITION_PROPERTIES.as_ptr(),
        user_data: ptr::null_mut(),
        path_xcb: Some(lvgl::lv_anim_path_step),
        time: 0,
        delay: 0,
    });

static INSTANT_TRANSITION_PROPERTIES: Shared<[lvgl::lv_style_const_prop_t; 2]> = Shared::new([
    lvgl::lv_style_const_prop_t {
        prop: lvgl::_lv_style_id_t_LV_STYLE_TRANSITION as lvgl::lv_style_prop_t,
        value: lvgl::lv_style_value_t {
            ptr: INSTANT_TRANSITION.get() as *const c_void,
        },
    },
    END,
]);

/// Replaces the theme's 150 ms transition: the new state shows in one frame.
static INSTANT: Shared<lvgl::lv_style_t> =
    Shared::new(constant_style(INSTANT_TRANSITION_PROPERTIES.get().cast()));

static UNBLENDED_PROPERTIES: Shared<[lvgl::lv_style_const_prop_t; 2]> = Shared::new([
    number(
        lvgl::_lv_style_id_t_LV_STYLE_BG_OPA,
        lvgl::LV_OPA_TRANSP as i32,
    ),
    END,
]);

/// Replaces the half-transparent black of a modal backdrop: blending it costs a
/// pass over the whole screen.
static UNBLENDED: Shared<lvgl::lv_style_t> =
    Shared::new(constant_style(UNBLENDED_PROPERTIES.get().cast()));

static UNCLIPPED_PROPERTIES: Shared<[lvgl::lv_style_const_prop_t; 2]> =
    Shared::new([number(lvgl::_lv_style_id_t_LV_STYLE_CLIP_CORNER, 0), END]);

/// Stops clipping children to rounded corners, which LVGL does by rendering the
/// top and bottom bands of the object into layers.
static UNCLIPPED: Shared<lvgl::lv_style_t> =
    Shared::new(constant_style(UNCLIPPED_PROPERTIES.get().cast()));

const MAIN: lvgl::lv_style_selector_t = lvgl::LV_PART_MAIN;
const ITEMS: lvgl::lv_style_selector_t = lvgl::LV_PART_ITEMS;
const SCROLLBAR: lvgl::lv_style_selector_t = lvgl::lv_part_t_LV_PART_SCROLLBAR;
const PRESSED: lvgl::lv_style_selector_t = lvgl::LV_STATE_PRESSED;
const SCROLLED: lvgl::lv_style_selector_t = lvgl::lv_state_t_LV_STATE_SCROLLED;
const ANY: lvgl::lv_style_selector_t = lvgl::lv_part_t_LV_PART_ANY | lvgl::lv_state_t_LV_STATE_ANY;

/// Share of the foreground that the theme mixes into a pressed key
/// (`LV_THEME_SHADCN_PRESS_MIX`).
const KEY_PRESS_MIX: u8 = 26;

/// What an object needs, from what the theme gives its class.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Nothing to adjust.
    Untouched,
    /// Looks different while pressed: buttons, list and menu entries.
    Pressable,
    /// Reacts to presses without a pressed look: inputs, toggles, sliders.
    Control,
    /// A plain object: a card or a layout container, which may scroll.
    Container,
    /// A text input, which is a control that scrolls its text.
    TextInput,
    /// Scrolls, and has no interaction styles of its own.
    Scrollable,
    /// A list or a table: scrolls, and clips its children to its corners.
    Collection,
    /// The on-screen keyboard.
    Keyboard,
    /// The dimmed area behind a message box.
    Backdrop,
}

fn kind_of(class: *const lvgl::lv_obj_class_t) -> Option<Kind> {
    let classes: [(*const lvgl::lv_obj_class_t, Kind); 25] = {
        [
            (&raw const lvgl::lv_label_class, Kind::Untouched),
            (&raw const lvgl::lv_obj_class, Kind::Container),
            (&raw const lvgl::lv_button_class, Kind::Pressable),
            (&raw const lvgl::lv_list_button_class, Kind::Pressable),
            (&raw const lvgl::lv_menu_cont_class, Kind::Pressable),
            (
                &raw const lvgl::lv_msgbox_footer_button_class,
                Kind::Pressable,
            ),
            (
                &raw const lvgl::lv_msgbox_header_button_class,
                Kind::Pressable,
            ),
            (&raw const lvgl::lv_textarea_class, Kind::TextInput),
            (&raw const lvgl::lv_spinbox_class, Kind::TextInput),
            (&raw const lvgl::lv_dropdown_class, Kind::Control),
            (&raw const lvgl::lv_roller_class, Kind::Control),
            (&raw const lvgl::lv_switch_class, Kind::Control),
            (&raw const lvgl::lv_checkbox_class, Kind::Control),
            (&raw const lvgl::lv_slider_class, Kind::Control),
            (&raw const lvgl::lv_arc_class, Kind::Control),
            (&raw const lvgl::lv_imagebutton_class, Kind::Control),
            (&raw const lvgl::lv_list_class, Kind::Collection),
            (&raw const lvgl::lv_table_class, Kind::Collection),
            (&raw const lvgl::lv_tileview_class, Kind::Scrollable),
            (&raw const lvgl::lv_tileview_tile_class, Kind::Scrollable),
            (&raw const lvgl::lv_menu_page_class, Kind::Scrollable),
            (&raw const lvgl::lv_dropdownlist_class, Kind::Scrollable),
            (&raw const lvgl::lv_msgbox_content_class, Kind::Scrollable),
            (&raw const lvgl::lv_keyboard_class, Kind::Keyboard),
            (&raw const lvgl::lv_msgbox_backdrop_class, Kind::Backdrop),
        ]
    };
    classes
        .iter()
        .find(|(known, _)| ptr::eq(*known, class))
        .map(|(_, kind)| *kind)
}

/// Adjusts `object` as an instance of `class`. Returns whether the class is one
/// the theme styles.
///
/// Safe to repeat on an object: styles and callbacks are added once.
unsafe fn adjust(object: *mut lvgl::lv_obj_t, class: *const lvgl::lv_obj_class_t) -> bool {
    let Some(kind) = kind_of(class) else {
        return false;
    };
    let light = LIGHT.load(Ordering::Relaxed);

    unsafe {
        // The theme adds its transition a second time for the pressed state. On
        // an object that does not look different pressed, that style only makes
        // LVGL redraw the whole object on every press and release: for a window
        // or a card, most of the screen. The copy for the default state already
        // covers both directions.
        if matches!(kind, Kind::Container | Kind::Control | Kind::TextInput) {
            lvgl::lv_obj_remove_theme(object, MAIN | PRESSED);
        }

        if kind == Kind::Keyboard {
            // Likewise, a style for pressed keys makes LVGL redraw the whole
            // keyboard for each key. The button matrix already redraws the one
            // key; `color_pressed_key` gives it the pressed color.
            lvgl::lv_obj_remove_theme(object, ITEMS | PRESSED);
            add_event_callback(
                object,
                color_pressed_key,
                lvgl::lv_event_code_t_LV_EVENT_DRAW_TASK_ADDED,
            );
            lvgl::lv_obj_set_send_draw_task_events(object, true);
        }

        if !light {
            return true;
        }

        match kind {
            Kind::Pressable => {
                add_style(object, INSTANT.get(), MAIN);
                add_style(object, INSTANT.get(), MAIN | PRESSED);
            }
            Kind::Control | Kind::TextInput | Kind::Container => {
                add_style(object, INSTANT.get(), MAIN);
            }
            Kind::Collection => add_style(object, UNCLIPPED.get(), MAIN),
            Kind::Backdrop => add_style(object, UNBLENDED.get(), MAIN),
            Kind::Scrollable | Kind::Keyboard | Kind::Untouched => {}
        }

        if matches!(
            kind,
            Kind::Container | Kind::TextInput | Kind::Scrollable | Kind::Collection
        ) {
            adjust_scrolling(object);
        }
    }
    true
}

/// Light rendering for an object that may scroll.
unsafe fn adjust_scrolling(object: *mut lvgl::lv_obj_t) {
    unsafe {
        // The theme makes the scrollbar more opaque during a scroll. LVGL
        // redraws the whole object when a style for a state comes or goes, so
        // that costs a full redraw of the scrolled object when the scroll ends.
        lvgl::lv_obj_remove_theme(object, SCROLLBAR | SCROLLED);
        add_event_callback(
            object,
            finish_scroll_at_once,
            lvgl::lv_event_code_t_LV_EVENT_SCROLL_BEGIN,
        );
    }
}

unsafe fn add_style(
    object: *mut lvgl::lv_obj_t,
    style: *const lvgl::lv_style_t,
    selector: lvgl::lv_style_selector_t,
) {
    // Adding a style that is already there with this selector moves it to the
    // top instead of adding it again.
    unsafe { lvgl::lv_obj_add_style(object, style, selector) };
}

unsafe fn add_event_callback(
    object: *mut lvgl::lv_obj_t,
    callback: unsafe extern "C" fn(*mut lvgl::lv_event_t),
    code: lvgl::lv_event_code_t,
) {
    unsafe {
        lvgl::lv_obj_remove_event_cb(object, Some(callback));
        lvgl::lv_obj_add_event_cb(object, Some(callback), code, ptr::null_mut());
    }
}

/// Snaps the scrolls that LVGL animates: scrolling to a position or an object,
/// settling on a snap point, returning from an overscroll. Each frame of such
/// an animation redraws the whole scrolled object. Scrolling under the finger
/// and its momentum do not come through here, and stay.
unsafe extern "C" fn finish_scroll_at_once(event: *mut lvgl::lv_event_t) {
    unsafe {
        // LVGL passes the animation it is about to start, or null for a scroll
        // without one.
        let animation = lvgl::lv_event_get_param(event).cast::<lvgl::lv_anim_t>();
        if !animation.is_null() {
            lvgl::lv_anim_set_duration(animation, 0);
            // LVGL's easing paths return the start value for a zero duration.
            lvgl::lv_anim_set_path_cb(animation, Some(lvgl::lv_anim_path_step));
        }
    }
}

/// Gives the pressed key of a keyboard the theme's pressed color.
unsafe extern "C" fn color_pressed_key(event: *mut lvgl::lv_event_t) {
    unsafe {
        let keyboard = lvgl::lv_event_get_current_target_obj(event);
        if !lvgl::lv_obj_has_state(keyboard, lvgl::LV_STATE_PRESSED as lvgl::lv_state_t) {
            return;
        }
        let task = lvgl::lv_event_get_draw_task(event);
        let fill = lvgl::lv_draw_task_get_fill_dsc(task);
        if fill.is_null()
            || (*fill).base.part != ITEMS
            || (*fill).base.id1 != lvgl::lv_buttonmatrix_get_selected_button(keyboard)
        {
            return;
        }
        let palette = lvgl::lv_theme_shadcn_get_palette(lvgl::lv_obj_get_display(keyboard));
        if let Some(palette) = palette.as_ref() {
            (*fill).color =
                lvgl::lv_color_mix(palette.secondary_fg, palette.secondary, KEY_PRESS_MIX);
        }
    }
}

/// Runs after the theme has styled a new object.
unsafe extern "C" fn apply_callback(_theme: *mut lvgl::lv_theme_t, object: *mut lvgl::lv_obj_t) {
    // For a class that inherits its base class's theme, LVGL calls the theme
    // once for each class with the object's class set to it.
    unsafe { adjust(object, lvgl::lv_obj_get_class(object)) };
}

/// Runs the animations before a frame is rendered, in light mode.
///
/// LVGL starts a transition on a state change and leaves even the only step of
/// [`INSTANT`] to the animation timer, which runs after the display's: each
/// state change would first be drawn once more with the old colors.
///
/// This also advances the momentum of a scroll, which LVGL slows down once for
/// each run of the animations instead of over time. On a display that renders
/// slower than the animation timer runs, a fling then covers the same distance
/// in half the frames.
unsafe extern "C" fn settle_transitions(_: *mut lvgl::lv_event_t) {
    if LIGHT.load(Ordering::Relaxed) {
        unsafe { lvgl::lv_anim_refr_now() };
    }
}

unsafe extern "C" fn delete_theme(event: *mut lvgl::lv_event_t) {
    unsafe { lvgl::lv_theme_delete(lvgl::lv_event_get_user_data(event).cast()) };
}

/// Chains [`apply_callback`] after the theme's own callback, once for a display.
///
/// The theme's helpers only act while the display's theme is the one the theme
/// created, so another theme cannot be put on top of it. Instead that theme
/// becomes the hook, and a copy that keeps its callback runs first as its parent.
pub(super) unsafe fn install(
    display: *mut lvgl::lv_display_t,
    theme: *mut lvgl::lv_theme_t,
) -> Result<()> {
    unsafe {
        let original = lvgl::lv_theme_create();
        if original.is_null() {
            return Err(Error::OutOfMemory);
        }
        lvgl::lv_theme_copy(original, theme);
        lvgl::lv_theme_set_parent(theme, original);
        lvgl::lv_theme_set_apply_cb(theme, Some(apply_callback));
        lvgl::lv_display_add_event_cb(
            display,
            Some(settle_transitions),
            lvgl::lv_event_code_t_LV_EVENT_REFR_START,
            ptr::null_mut(),
        );
        // The theme frees itself with the display, but not the copy.
        lvgl::lv_display_add_event_cb(
            display,
            Some(delete_theme),
            lvgl::lv_event_code_t_LV_EVENT_DELETE,
            original.cast(),
        );
    }
    Ok(())
}

unsafe extern "C" fn adjust_existing(
    object: *mut lvgl::lv_obj_t,
    _: *mut c_void,
) -> lvgl::lv_obj_tree_walk_res_t {
    unsafe {
        // Outside the theme's own pass, try the object's class and then the
        // classes it derives from, as the theme did when it styled it.
        let mut class = lvgl::lv_obj_get_class(object);
        while !class.is_null() && !adjust(object, class) {
            class = (*class).base_class;
        }
    }
    lvgl::lv_obj_tree_walk_res_t_LV_OBJ_TREE_WALK_NEXT
}

unsafe extern "C" fn restore_existing(
    object: *mut lvgl::lv_obj_t,
    _: *mut c_void,
) -> lvgl::lv_obj_tree_walk_res_t {
    unsafe {
        for style in [INSTANT.get(), UNBLENDED.get(), UNCLIPPED.get()] {
            lvgl::lv_obj_remove_style(object, style, ANY);
        }
        lvgl::lv_obj_remove_event_cb(object, Some(finish_scroll_at_once));
    }
    lvgl::lv_obj_tree_walk_res_t_LV_OBJ_TREE_WALK_NEXT
}

/// Select the rendering mode of the theme on `display`.
///
/// The mode is shared by every display. Objects created from now on follow it,
/// and so do the objects that `display` already shows, with one exception:
/// those created in light mode keep their scrollbar unchanged during a scroll.
///
/// # Safety
/// `display` must be live and the caller must hold the LVGL lock.
pub unsafe fn set_display_rendering_mode(
    display: *mut lvgl::lv_display_t,
    mode: RenderingMode,
) -> Result<()> {
    unsafe {
        if !lvgl::lv_theme_shadcn_is_active(display) {
            return Err(Error::NotAvailable);
        }
        let light = mode == RenderingMode::Light;
        LIGHT.store(light, Ordering::Relaxed);

        let callback: lvgl::lv_obj_tree_walk_cb_t = if light {
            Some(adjust_existing)
        } else {
            Some(restore_existing)
        };
        for root in [
            lvgl::lv_display_get_screen_active(display),
            lvgl::lv_display_get_layer_top(display),
            lvgl::lv_display_get_layer_sys(display),
            lvgl::lv_display_get_layer_bottom(display),
        ] {
            if !root.is_null() {
                lvgl::lv_obj_tree_walk(root, callback, ptr::null_mut());
            }
        }
    }
    Ok(())
}

/// The rendering mode that [`set_display_rendering_mode`] last selected.
pub fn get_rendering_mode() -> RenderingMode {
    if LIGHT.load(Ordering::Relaxed) {
        RenderingMode::Light
    } else {
        RenderingMode::Default
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
