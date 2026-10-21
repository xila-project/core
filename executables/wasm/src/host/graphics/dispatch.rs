use super::{
    Function, GraphicsState, ResultKind,
    error::{GraphicsError, Result},
    marshal::{
        ButtonMap, MAX_STRING_BYTES, MAX_WIDGET_OPTIONS_BYTES, read_c_string, read_textarea_string,
        read_widget_options,
    },
    objects::Kind,
};
use crate::host::translation::{GuestMemory, GuestPointer, GuestSlice, WasmUsize};
use core::{ffi::CStr, ptr::NonNull};
use xila::graphics::{Color, Window, lvgl};

pub(super) fn signed_word(value: WasmUsize) -> Result<i32> {
const MAX_RANGE_VALUE: i32 = 1_000_000;

    let narrowed = value as u32 as i32;
    if narrowed as WasmUsize == value {
        Ok(narrowed)
    } else {
        Err(GraphicsError::InvalidValue)
    }
}

fn copy_selected_option(
    memory: &mut GuestMemory<'_>,
    options: *const core::ffi::c_char,
    selected: u32,
    output: WasmUsize,
    capacity: WasmUsize,
    result: WasmUsize,
) -> Result<i32> {
    if options.is_null() {
        return Err(GraphicsError::NativeFailure);
    }
    let options = unsafe { CStr::from_ptr(options) }.to_bytes();
    if options.len() >= MAX_WIDGET_OPTIONS_BYTES {
        return Err(GraphicsError::ResourceLimit);
    }
    let option = options
        .split(|byte| *byte == b'\n')
        .nth(selected as usize)
        .ok_or(GraphicsError::InvalidValue)?;
    let required = option.len() + 1;
    if required > MAX_STRING_BYTES {
        return Err(GraphicsError::ResourceLimit);
    }
    if usize::try_from(capacity).map_err(|_| GraphicsError::InvalidValue)? < required {
        return Err(GraphicsError::InvalidValue);
    }
    let output_end = output
        .checked_add(required as WasmUsize)
        .ok_or(GraphicsError::InvalidMemory)?;
    let result_end = result
        .checked_add(core::mem::size_of::<i32>() as WasmUsize)
        .ok_or(GraphicsError::InvalidMemory)?;
    if output < result_end && result < output_end {
        return Err(GraphicsError::InvalidMemory);
    }
    let destination = memory
        .bytes_mut(GuestSlice::new(output, required as WasmUsize))
        .ok_or(GraphicsError::InvalidMemory)?;
    destination[..option.len()].copy_from_slice(option);
    destination[option.len()] = 0;
    i32::try_from(option.len()).map_err(|_| GraphicsError::ResourceLimit)
}

pub struct GraphicsCall {
    pub function: i32,
    pub arguments: [WasmUsize; 7],
    pub count: i32,
    pub result: WasmUsize,
}

// Checked narrowing is required for memory_64 and intentionally a no-op for memory_32.
#[allow(clippy::useless_conversion)]
pub fn dispatch(
    state: &mut GraphicsState,
    memory: &mut GuestMemory<'_>,
    call: GraphicsCall,
) -> Result<()> {
    let function = Function::from_id(call.function).ok_or(GraphicsError::Unsupported)?;
    if function.argument_count() != call.count {
        return Err(GraphicsError::InvalidArgumentsCount);
    }
    let manager = state.manager.ok_or(GraphicsError::Unavailable)?;
    match function.result_kind() {
        ResultKind::None => (),
        ResultKind::Word => {
            memory
                .validate_pointer(GuestPointer::<WasmUsize>::new(call.result))
                .ok_or(GraphicsError::InvalidMemory)?;
        }
        ResultKind::U32 | ResultKind::I32 => {
            memory
                .validate_pointer(GuestPointer::<u32>::new(call.result))
                .ok_or(GraphicsError::InvalidMemory)?;
        }
    }
    let _lock = manager.synchronous_lock();
    state.reap_deleted_maps();
    if super::dispatch_generated(state, memory, &call, function)? {
        return Ok(());
    }
    let [a, b, c, ..] = call.arguments;
    match function {
        Function::ObjectSetPadding => {
            let object = state.object(a, None)?;
            let padding = i32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            if !(0..=64).contains(&padding) {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe {
                lvgl::lv_obj_set_style_pad_all(object, padding, 0);
                lvgl::lv_obj_set_style_pad_row(object, padding, 0);
                lvgl::lv_obj_set_style_pad_column(object, padding, 0);
            }
        }
        Function::ObjectAlign => {
            let object = state.object(a, None)?;
            if b > lvgl::lv_align_t_LV_ALIGN_OUT_RIGHT_BOTTOM as WasmUsize {
                return Err(GraphicsError::InvalidValue);
            }
            let x = signed_word(c)?;
            let y = signed_word(call.arguments[3])?;
            unsafe { lvgl::lv_obj_align(object, b as _, x, y) };
        }
        Function::ObjectUpdateLayout => {
            let object = state.object(a, None)?;
            unsafe { lvgl::lv_obj_update_layout(object) };
        }
        Function::ObjectSetBackground => {
            let object = state.object(a, None)?;
            let rgb = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let opacity = u8::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            let selector =
                u32::try_from(call.arguments[3]).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if rgb > 0x00ff_ffff {
                return Err(GraphicsError::InvalidValue);
            }
            let color = unsafe { lvgl::lv_color_hex(rgb) };
            unsafe {
                lvgl::lv_obj_set_style_bg_color(object, color, selector);
                lvgl::lv_obj_set_style_bg_opa(object, opacity, selector);
            }
        }
        Function::ObjectSetTextColor => {
            let object = state.object(a, None)?;
            let rgb = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let selector = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if rgb > 0x00ff_ffff {
                return Err(GraphicsError::InvalidValue);
            }
            let color = unsafe { lvgl::lv_color_hex(rgb) };
            unsafe { lvgl::lv_obj_set_style_text_color(object, color, selector) };
        }
        Function::ObjectSetBorder => {
            let object = state.object(a, None)?;
            let rgb = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let width = signed_word(c)?;
            let side = u32::try_from(call.arguments[3]).map_err(|_| GraphicsError::InvalidValue)?;
            let selector =
                u32::try_from(call.arguments[4]).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if rgb > 0x00ff_ffff || !(0..=64).contains(&width) || side & !0x1f != 0 {
                return Err(GraphicsError::InvalidValue);
            }
            let color = unsafe { lvgl::lv_color_hex(rgb) };
            unsafe {
                lvgl::lv_obj_set_style_border_color(object, color, selector);
                lvgl::lv_obj_set_style_border_width(object, width, selector);
                lvgl::lv_obj_set_style_border_side(object, side, selector);
            }
        }
        Function::ObjectSetRadius => {
            let object = state.object(a, None)?;
            let radius = signed_word(b)?;
            let selector = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if !(0..=1024).contains(&radius) {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_style_radius(object, radius, selector) };
        }
        Function::ObjectSetTextAlign => {
            let object = state.object(a, None)?;
            let align = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let selector = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if align > lvgl::lv_text_align_t_LV_TEXT_ALIGN_RIGHT {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_style_text_align(object, align, selector) };
        }
        Function::ObjectSetFont => {
            let object = state.object(a, None)?;
            let font = match b {
                0 => xila::graphics::fonts::get_font_small(),
                1 => xila::graphics::fonts::get_font_medium(),
                2 => xila::graphics::fonts::get_font_large(),
                3 => xila::graphics::fonts::get_font_extra_large(),
                4 => xila::graphics::fonts::get_font_monospace_small(),
                5 => xila::graphics::fonts::get_font_monospace_medium(),
                _ => return Err(GraphicsError::InvalidValue),
            };
            let selector = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            unsafe { lvgl::lv_obj_set_style_text_font(object, font, selector) };
        }
        Function::ObjectSetSurface => {
            let object = state.object(a, None)?;
            let surface = match b {
                0 => xila::graphics::theme::Surface::Background,
                1 => xila::graphics::theme::Surface::Card,
                2 => xila::graphics::theme::Surface::Popover,
                3 => xila::graphics::theme::Surface::Muted,
                _ => return Err(GraphicsError::InvalidValue),
            };
            unsafe { xila::graphics::theme::set_surface(object, surface) };
        }
        Function::ObjectSetMinMaxSize => {
            let object = state.object(a, None)?;
            let min_width = signed_word(b)?;
            let max_width = signed_word(c)?;
            let min_height = signed_word(call.arguments[3])?;
            let max_height = signed_word(call.arguments[4])?;
            let selector =
                u32::try_from(call.arguments[5]).map_err(|_| GraphicsError::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if !(0..=16_384).contains(&min_width)
                || !(0..=16_384).contains(&max_width)
                || !(0..=16_384).contains(&min_height)
                || !(0..=16_384).contains(&max_height)
                || min_width > max_width
                || min_height > max_height
            {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe {
                lvgl::lv_obj_set_style_min_width(object, min_width, selector);
                lvgl::lv_obj_set_style_max_width(object, max_width, selector);
                lvgl::lv_obj_set_style_min_height(object, min_height, selector);
                lvgl::lv_obj_set_style_max_height(object, max_height, selector);
            }
        }
        Function::RangeSetRange => {
            let object = state.object(a, None)?;
            let minimum = signed_word(b)?;
            let maximum = signed_word(c)?;
            if minimum >= maximum || minimum < -MAX_RANGE_VALUE || maximum > MAX_RANGE_VALUE {
                return Err(GraphicsError::InvalidValue);
            }
            match state.kind(a)? {
                Kind::Arc => unsafe { lvgl::lv_arc_set_range(object, minimum, maximum) },
                Kind::Bar => unsafe { lvgl::lv_bar_set_range(object, minimum, maximum) },
                Kind::Slider => unsafe { lvgl::lv_slider_set_range(object, minimum, maximum) },
                _ => return Err(GraphicsError::InvalidHandle),
            }
        }
        Function::RangeSetValue => {
            let object = state.object(a, None)?;
            let value = signed_word(b)?;
            if value < -MAX_RANGE_VALUE || value > MAX_RANGE_VALUE || c > 1 {
                return Err(GraphicsError::InvalidValue);
            }
            match state.kind(a)? {
                Kind::Arc => {
                    if c != 0 {
                        return Err(GraphicsError::InvalidValue);
                    }
                    let minimum = unsafe { lvgl::lv_arc_get_min_value(object) };
                    let maximum = unsafe { lvgl::lv_arc_get_max_value(object) };
                    if !(minimum..=maximum).contains(&value) {
                        return Err(GraphicsError::InvalidValue);
                    }
                    unsafe { lvgl::lv_arc_set_value(object, value) };
                }
                Kind::Bar => {
                    let minimum = unsafe { lvgl::lv_bar_get_min_value(object) };
                    let maximum = unsafe { lvgl::lv_bar_get_max_value(object) };
                    if !(minimum..=maximum).contains(&value) {
                        return Err(GraphicsError::InvalidValue);
                    }
                    unsafe { lvgl::lv_bar_set_value(object, value, c != 0) };
                }
                Kind::Slider => {
                    let minimum = unsafe { lvgl::lv_slider_get_min_value(object) };
                    let maximum = unsafe { lvgl::lv_slider_get_max_value(object) };
                    if !(minimum..=maximum).contains(&value) {
                        return Err(GraphicsError::InvalidValue);
                    }
                    unsafe { lvgl::lv_slider_set_value(object, value, c != 0) };
                }
                _ => return Err(GraphicsError::InvalidHandle),
            }
        }
        Function::RangeGetValue => {
            let object = state.object(a, None)?;
            let value = match state.kind(a)? {
                Kind::Arc => unsafe { lvgl::lv_arc_get_value(object) },
                Kind::Bar => unsafe { lvgl::lv_bar_get_value(object) },
                Kind::Slider => unsafe { lvgl::lv_slider_get_value(object) },
                _ => return Err(GraphicsError::InvalidHandle),
            };
            memory
                .write(GuestPointer::<i32>::new(call.result), value)
                .expect("validated result");
        }
        Function::RangeGetStartValue => {
            let object = state.object(a, None)?;
            let value = match state.kind(a)? {
                Kind::Bar => unsafe { lvgl::lv_bar_get_start_value(object) },
                Kind::Slider => unsafe { lvgl::lv_slider_get_left_value(object) },
                _ => return Err(GraphicsError::InvalidHandle),
            };
            memory
                .write(GuestPointer::<i32>::new(call.result), value)
                .expect("validated result");
        }
        Function::RangeSetMode => {
            let object = state.object(a, None)?;
            if b > 2 {
                return Err(GraphicsError::InvalidValue);
            }
            match state.kind(a)? {
                Kind::Arc => unsafe { lvgl::lv_arc_set_mode(object, b as _) },
                Kind::Bar => unsafe { lvgl::lv_bar_set_mode(object, b as _) },
                Kind::Slider => unsafe { lvgl::lv_slider_set_mode(object, b as _) },
                _ => return Err(GraphicsError::InvalidHandle),
            }
        }
        Function::RangeSetOrientation => {
            let object = state.object(a, None)?;
            if b > 2 {
                return Err(GraphicsError::InvalidValue);
            }
            match state.kind(a)? {
                Kind::Bar => unsafe { lvgl::lv_bar_set_orientation(object, b as _) },
                Kind::Slider => unsafe { lvgl::lv_slider_set_orientation(object, b as _) },
                Kind::Switch => unsafe { lvgl::lv_switch_set_orientation(object, b as _) },
                _ => return Err(GraphicsError::InvalidHandle),
            }
        }
        Function::SpinnerSetAnimation => {
            let spinner = state.object(a, Some(Kind::Spinner))?;
            let duration = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let sweep = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            if duration > 600_000 || sweep > 360 {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_spinner_set_anim_params(spinner, duration, sweep) };
        }
        Function::SpinnerGetAnimationDuration => {
            let spinner = state.object(a, Some(Kind::Spinner))?;
            let duration = unsafe { lvgl::lv_spinner_get_anim_duration(spinner) };
            memory
                .write(GuestPointer::<u32>::new(call.result), duration)
                .expect("validated result");
        }
        Function::SpinnerGetArcSweep => {
            let spinner = state.object(a, Some(Kind::Spinner))?;
            let sweep = unsafe { lvgl::lv_spinner_get_arc_sweep(spinner) };
            memory
                .write(GuestPointer::<u32>::new(call.result), sweep)
                .expect("validated result");
        }
        Function::DropdownSetOptions => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            let options = read_widget_options(memory, b)?;
            let retained_bytes = options.to_bytes_with_nul().len();
            state.validate_widget_options_size(a, retained_bytes)?;
            // LVGL's dynamic setter copies options into its own allocation.
            unsafe { lvgl::lv_dropdown_set_options(dropdown, options.as_ptr()) };
            state.commit_widget_options_size(a, retained_bytes)?;
        }
        Function::DropdownSetSelected => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            let selected = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let count = unsafe { lvgl::lv_dropdown_get_option_count(dropdown) };
            if selected >= count {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_dropdown_set_selected(dropdown, selected) };
        }
        Function::DropdownGetSelected | Function::DropdownGetOptionCount => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            let value = unsafe {
                if function == Function::DropdownGetSelected {
                    lvgl::lv_dropdown_get_selected(dropdown)
                } else {
                    lvgl::lv_dropdown_get_option_count(dropdown)
                }
            };
            memory
                .write(GuestPointer::<u32>::new(call.result), value)
                .expect("validated result");
        }
        Function::DropdownSetSelectedHighlight => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            if b > 1 {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_dropdown_set_selected_highlight(dropdown, b != 0) };
        }
        Function::DropdownOpen | Function::DropdownClose => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            unsafe {
                if function == Function::DropdownOpen {
                    lvgl::lv_dropdown_open(dropdown);
                } else {
                    lvgl::lv_dropdown_close(dropdown);
                }
            }
        }
        Function::DropdownGetSelectedString => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            let selected = unsafe { lvgl::lv_dropdown_get_selected(dropdown) };
            let options = unsafe { lvgl::lv_dropdown_get_options(dropdown) };
            let capacity = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            if capacity as usize > MAX_STRING_BYTES {
                return Err(GraphicsError::ResourceLimit);
            }
            let length = copy_selected_option(
                memory,
                options,
                selected,
                b,
                capacity as WasmUsize,
                call.result,
            )?;
            memory
                .write(GuestPointer::<i32>::new(call.result), length)
                .expect("validated result");
        }
        Function::DropdownGetHighlight | Function::DropdownIsOpenState => {
            let dropdown = state.object(a, Some(Kind::Dropdown))?;
            let value = unsafe {
                if function == Function::DropdownGetHighlight {
                    lvgl::lv_dropdown_get_selected_highlight(dropdown)
                } else {
                    lvgl::lv_dropdown_is_open(dropdown)
                }
            } as u32;
            memory
                .write(GuestPointer::<u32>::new(call.result), value)
                .expect("validated result");
        }
        Function::RollerSetOptions => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let options = read_widget_options(memory, b)?;
            if c > 1 {
                return Err(GraphicsError::InvalidValue);
            }
            let bytes = options.to_bytes_with_nul().len();
            if c == lvgl::lv_roller_mode_t_LV_ROLLER_MODE_INFINITE as WasmUsize
                && bytes > super::marshal::MAX_ROLLER_INFINITE_OPTIONS_BYTES
            {
                return Err(GraphicsError::ResourceLimit);
            }
            let retained_bytes = bytes
                .checked_mul(
                    if c == lvgl::lv_roller_mode_t_LV_ROLLER_MODE_INFINITE as WasmUsize {
                        15
                    } else {
                        1
                    },
                )
                .ok_or(GraphicsError::ResourceLimit)?;
            state.validate_widget_options_size(a, retained_bytes)?;
            // LVGL copies the option string, including in infinite mode.
            unsafe { lvgl::lv_roller_set_options(roller, options.as_ptr(), c as _) };
            state.commit_widget_options_size(a, retained_bytes)?;
        }
        Function::RollerSetSelected => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let selected = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let animated = call.arguments[2];
            if animated > 1 || selected >= unsafe { lvgl::lv_roller_get_option_count(roller) } {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_roller_set_selected(roller, selected, animated != 0) };
        }
        Function::RollerGetSelected | Function::RollerGetOptionCount => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let value = unsafe {
                if function == Function::RollerGetSelected {
                    lvgl::lv_roller_get_selected(roller)
                } else {
                    lvgl::lv_roller_get_option_count(roller)
                }
            };
            memory
                .write(GuestPointer::<u32>::new(call.result), value)
                .expect("validated result");
        }
        Function::RollerSetVisibleRowCount => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let rows = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            if !(1..=32).contains(&rows) {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_roller_set_visible_row_count(roller, rows) };
        }
        Function::RollerGetSelectedString => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let selected = unsafe { lvgl::lv_roller_get_selected(roller) };
            let options = unsafe { lvgl::lv_roller_get_options(roller) };
            let capacity = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            if capacity as usize > MAX_STRING_BYTES {
                return Err(GraphicsError::ResourceLimit);
            }
            let length = copy_selected_option(
                memory,
                options,
                selected,
                b,
                capacity as WasmUsize,
                call.result,
            )?;
            memory
                .write(GuestPointer::<i32>::new(call.result), length)
                .expect("validated result");
        }
        Function::ObjectSetFlexAlign => {
            let object = state.object(a, None)?;
            let main = b;
            let cross = c;
            let track = call.arguments[3];
            if [main, cross, track].iter().any(|&align| {
                align > lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN as WasmUsize
            }) {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_flex_align(object, main as _, cross as _, track as _) };
        }
        Function::ObjectSetLayout => {
            let object = state.object(a, None)?;
            if !matches!(b, 0 | 1 | 2) {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_layout(object, b as _) };
        }
        Function::GridFr => {
            let weight = u8::try_from(a).map_err(|_| GraphicsError::InvalidValue)?;
            if !(0..=99).contains(&weight) {
                return Err(GraphicsError::InvalidValue);
            }
            let descriptor = unsafe { lvgl::lv_grid_fr(weight) };
            memory
                .write(GuestPointer::<i32>::new(call.result), descriptor)
                .expect("validated result");
        }
        Function::ObjectSetGridDscArray => {
            state.object(a, None)?;
            let tracks = super::marshal::GridTracks::decode(
                memory,
                b,
                c,
                lvgl::LV_GRID_TEMPLATE_LAST as i32,
                lvgl::LV_GRID_CONTENT as i32,
            )?;
            state.set_grid_tracks(a, tracks)?;
        }
        Function::ObjectSetGridAlign => {
            let object = state.object(a, None)?;
            let column = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let row = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            if column > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
                || row > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
            {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_grid_align(object, column, row) };
        }
        Function::ObjectSetGridCell => {
            let object = state.object(a, None)?;
            let column_align = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let column = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            let column_span =
                u32::try_from(call.arguments[3]).map_err(|_| GraphicsError::InvalidValue)?;
            let row_align =
                u32::try_from(call.arguments[4]).map_err(|_| GraphicsError::InvalidValue)?;
            let row = u32::try_from(call.arguments[5]).map_err(|_| GraphicsError::InvalidValue)?;
            let row_span =
                u32::try_from(call.arguments[6]).map_err(|_| GraphicsError::InvalidValue)?;
            if column_align > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
                || row_align > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
                || column >= (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
                || row >= (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
                || column_span == 0
                || column_span > (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
                || row_span == 0
                || row_span > (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
            {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe {
                lvgl::lv_obj_set_grid_cell(
                    object,
                    column_align,
                    column as i32,
                    column_span as i32,
                    row_align,
                    row as i32,
                    row_span as i32,
                )
            };
        }
        Function::WindowCreate
        | Function::ObjectCreate
        | Function::LabelCreate
        | Function::ButtonCreate
        | Function::CheckboxCreate
        | Function::TabviewCreate
        | Function::ButtonmatrixCreate
        | Function::TextareaCreate
        | Function::KeyboardCreate
        | Function::ArcCreate
        | Function::BarCreate
        | Function::SliderCreate
        | Function::SwitchCreate
        | Function::SpinnerCreate
        | Function::DropdownCreate
        | Function::RollerCreate => {
            state.ensure_capacity()?;
            let (pointer, kind, parent) = if function == Function::WindowCreate {
                let window = xila::task::block_on(manager.create_window())
                    .map_err(|_| GraphicsError::NativeFailure)?;
                let window: NonNull<Window> = window.into();
                (window.as_ptr().cast(), Kind::Window, None)
            } else {
                let parent = state.object(a, None)?;
                let (pointer, kind) = unsafe {
                    match function {
                        Function::ObjectCreate => (lvgl::lv_obj_create(parent), Kind::Object),
                        Function::TextareaCreate => {
                            (lvgl::lv_textarea_create(parent), Kind::Textarea)
                        }
                        Function::KeyboardCreate => {
                            (lvgl::lv_keyboard_create(parent), Kind::Keyboard)
                        }
                        Function::LabelCreate => (lvgl::lv_label_create(parent), Kind::Label),
                        Function::ButtonCreate => (lvgl::lv_button_create(parent), Kind::Object),
                        Function::CheckboxCreate => {
                            (lvgl::lv_checkbox_create(parent), Kind::Checkbox)
                        }
                        Function::TabviewCreate => (lvgl::lv_tabview_create(parent), Kind::TabView),
                        Function::ButtonmatrixCreate => {
                            (lvgl::lv_buttonmatrix_create(parent), Kind::ButtonMatrix)
                        }
                        Function::ArcCreate => (lvgl::lv_arc_create(parent), Kind::Arc),
                        Function::BarCreate => (lvgl::lv_bar_create(parent), Kind::Bar),
                        Function::SliderCreate => (lvgl::lv_slider_create(parent), Kind::Slider),
                        Function::SwitchCreate => (lvgl::lv_switch_create(parent), Kind::Switch),
                        Function::SpinnerCreate => (lvgl::lv_spinner_create(parent), Kind::Spinner),
                        Function::DropdownCreate => {
                            (lvgl::lv_dropdown_create(parent), Kind::Dropdown)
                        }
                        Function::RollerCreate => (lvgl::lv_roller_create(parent), Kind::Roller),
                        _ => unreachable!(),
                    }
                };
                (pointer, kind, Some(a))
            };
            let handle = state.insert(pointer, kind, parent)?;
            memory
                .write(GuestPointer::new(call.result), handle)
                .expect("validated result");
        }
        Function::TabviewAddTab => {
            state.ensure_capacity()?;
            let parent = state.object(a, Some(Kind::TabView))?;
            let text = read_c_string(memory, b)?;
            // LVGL copies the tab caption; it never retains guest memory.
            let tab = unsafe { lvgl::lv_tabview_add_tab(parent, text.as_ptr()) };
            let handle = state.insert(tab, Kind::Object, Some(a))?;
            memory
                .write(GuestPointer::new(call.result), handle)
                .expect("validated result");
        }
        Function::ObjectDelete => {
            unsafe { lvgl::lv_obj_delete(state.object(a, None)?) };
            state.reap_deleted_maps();
        }
        Function::WindowSetIcon => {
            let pointer = state.object(a, Some(Kind::Window))?;
            let text = read_c_string(memory, b)?
                .to_str()
                .map_err(|_| GraphicsError::InvalidValue)?;
            let color = u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            let color = lvgl::lv_color_t {
                red: (color >> 16) as u8,
                green: (color >> 8) as u8,
                blue: color as u8,
            };
            let mut window =
                unsafe { Window::from_raw(pointer) }.ok_or(GraphicsError::InvalidHandle)?;
            unsafe {
                window
                    .as_mut()
                    .set_icon(text, Color::from_lvgl_color(color))
            };
        }
        Function::WindowPopEvent | Function::WindowPopPointerEvent => {
            let pointer = state.object(a, Some(Kind::Window))?;
            memory
                .validate_pointer(GuestPointer::<u32>::new(b))
                .ok_or(GraphicsError::InvalidMemory)?;
            memory
                .validate_pointer(GuestPointer::<WasmUsize>::new(c))
                .ok_or(GraphicsError::InvalidMemory)?;
            if function == Function::WindowPopPointerEvent {
                for &offset in &call.arguments[3..7] {
                    memory
                        .validate_pointer(GuestPointer::<i32>::new(offset))
                        .ok_or(GraphicsError::InvalidMemory)?;
                }
            }
            let mut window =
                unsafe { Window::from_raw(pointer) }.ok_or(GraphicsError::InvalidHandle)?;
            let event = unsafe { window.as_mut().pop_event() };
            if function == Function::WindowPopPointerEvent {
                let point = event.as_ref().and_then(|event| event.pointer).unwrap_or([
                    i32::MIN,
                    i32::MIN,
                    0,
                    0,
                ]);
                for (&offset, value) in call.arguments[3..7].iter().zip(point) {
                    memory
                        .write(GuestPointer::new(offset), value)
                        .expect("validated pointer output");
                }
            }
            let (code, target) = event.map_or((0, 0), |event| {
                (
                    super::events::guest_code(event.code),
                    state.handle_for(event.target),
                )
            });
            memory
                .write(GuestPointer::new(b), code)
                .expect("validated event code");
            memory
                .write(GuestPointer::new(c), target)
                .expect("validated event target");
        }
        Function::IndexedCanvasCreate => {
            state.ensure_capacity()?;
            let parent = state.object(a, None)?;
            let width = u16::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
            let height = u16::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
            let canvas = super::canvas::IndexedCanvas::new(width as usize, height as usize)?;
            let object = unsafe { lvgl::lv_obj_create(parent) };
            let handle = state.insert(object, Kind::IndexedCanvas, Some(a))?;
            if let Err(error) = state.install_canvas(handle, canvas) {
                unsafe {
                    lvgl::lv_obj_delete(object);
                }
                state.reap_deleted_maps();
                return Err(error);
            }
            memory
                .write(GuestPointer::new(call.result), handle)
                .expect("validated result");
        }
        Function::IndexedCanvasSetPixels => {
            let object = state.object(a, Some(Kind::IndexedCanvas))?;
            let colors = call.arguments[4];
            if colors == 0 || colors > 256 {
                return Err(GraphicsError::InvalidValue);
            }
            let pixels = memory
                .bytes(GuestSlice::new(b, c))
                .ok_or(GraphicsError::InvalidMemory)?;
            let palette = memory
                .values(GuestSlice::<u32>::new(call.arguments[3], colors))
                .ok_or(GraphicsError::InvalidMemory)?;
            state.canvas_mut(a).upload(pixels, palette)?;
            unsafe {
                lvgl::lv_obj_invalidate(object);
            }
        }
        Function::TextareaSetOneLine | Function::TextareaSetMaxLength => {
            let object = state.object(a, Some(Kind::Textarea))?;
            unsafe {
                if function == Function::TextareaSetOneLine {
                    if b > 1 {
                        return Err(GraphicsError::InvalidValue);
                    }
                    lvgl::lv_textarea_set_one_line(object, b != 0);
                } else {
                    let length = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                    if length > 4096 {
                        return Err(GraphicsError::ResourceLimit);
                    }
                    lvgl::lv_textarea_set_max_length(object, length);
                }
            }
        }
        Function::TextareaGetText | Function::TextareaGetTextLength => {
            let object = state.object(a, Some(Kind::Textarea))?;
            let text = unsafe { core::ffi::CStr::from_ptr(lvgl::lv_textarea_get_text(object)) };
            let bytes = text.to_bytes_with_nul();
            let length =
                i32::try_from(bytes.len() - 1).map_err(|_| GraphicsError::ResourceLimit)?;
            if function == Function::TextareaGetText {
                if c < bytes.len() as WasmUsize {
                    return Err(GraphicsError::InvalidValue);
                }
                let output = memory
                    .bytes_mut(GuestSlice::new(b, c))
                    .ok_or(GraphicsError::InvalidMemory)?;
                output[..bytes.len()].copy_from_slice(bytes);
            }
            memory
                .write(GuestPointer::new(call.result), length)
                .expect("validated result");
        }
        Function::KeyboardSetTextarea => {
            let keyboard = state.object(a, Some(Kind::Keyboard))?;
            let textarea = if b == 0 {
                core::ptr::null_mut()
            } else {
                state.object(b, Some(Kind::Textarea))?
            };
            unsafe {
                lvgl::lv_keyboard_set_textarea(keyboard, textarea);
            }
        }
        Function::Percentage => {
            memory
                .write(GuestPointer::new(call.result), unsafe {
                    lvgl::lv_pct(a as i32)
                })
                .expect("validated result");
        }
        Function::LabelSetText | Function::CheckboxSetText | Function::TextareaSetText => {
            let kind = if function == Function::LabelSetText {
                Kind::Label
            } else if function == Function::CheckboxSetText {
                Kind::Checkbox
            } else {
                Kind::Textarea
            };
            let pointer = state.object(a, Some(kind))?;
            let text = if function == Function::TextareaSetText {
                read_textarea_string(memory, b)?
            } else {
                read_c_string(memory, b)?
            };
            // Both setters copy synchronously; neither retains guest bytes.
            unsafe {
                if function == Function::LabelSetText {
                    lvgl::lv_label_set_text(pointer, text.as_ptr());
                } else if function == Function::CheckboxSetText {
                    lvgl::lv_checkbox_set_text(pointer, text.as_ptr());
                } else {
                    lvgl::lv_textarea_set_text(pointer, text.as_ptr());
                }
            }
        }
        Function::TabviewGetTabActive | Function::ObjectGetState => {
            let kind = if function == Function::TabviewGetTabActive {
                Some(Kind::TabView)
            } else {
                None
            };
            let pointer = state.object(a, kind)?;
            let value = unsafe {
                if function == Function::TabviewGetTabActive {
                    lvgl::lv_tabview_get_tab_active(pointer)
                } else {
                    lvgl::lv_obj_get_state(pointer)
                }
            };
            memory
                .write(GuestPointer::new(call.result), value)
                .expect("validated result");
        }
        Function::ButtonmatrixSetMap => {
            state.object(a, Some(Kind::ButtonMatrix))?;
            let map = ButtonMap::decode(memory, b)?;
            state.set_map(a, map);
        }
        Function::ButtonmatrixGetSelectedButton => {
            let pointer = state.object(a, Some(Kind::ButtonMatrix))?;
            let selected = unsafe { lvgl::lv_buttonmatrix_get_selected_button(pointer) };
            memory
                .write(GuestPointer::new(call.result), selected)
                .expect("validated result");
        }
        Function::ObjectSetFlexFlow
        | Function::ObjectSetFlexGrow
        | Function::ObjectAddState
        | Function::ObjectRemoveState
        | Function::ObjectAddFlag
        | Function::ObjectRemoveFlag => {
            let pointer = state.object(a, None)?;
            unsafe {
                match function {
                    Function::ObjectSetFlexFlow => {
                        if !matches!(b, 0 | 1 | 4 | 5 | 8 | 9 | 12 | 13) {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_obj_set_flex_flow(pointer, b as _);
                    }
                    Function::ObjectSetFlexGrow => lvgl::lv_obj_set_flex_grow(
                        pointer,
                        u8::try_from(b).map_err(|_| GraphicsError::InvalidValue)?,
                    ),
                    Function::ObjectAddState => {
                        let state = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        if state & !0xF3FD != 0 {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_obj_add_state(pointer, state);
                    }
                    Function::ObjectRemoveState => {
                        let state = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        if state & !0xF3FD != 0 {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_obj_remove_state(pointer, state);
                    }
                    Function::ObjectRemoveFlag => lvgl::lv_obj_remove_flag(
                        pointer,
                        u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?,
                    ),
                    Function::ObjectAddFlag => lvgl::lv_obj_add_flag(
                        pointer,
                        u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?,
                    ),
                    _ => unreachable!(),
                }
            }
        }
        _ => return Err(GraphicsError::Unsupported),
    }
    Ok(())
}
