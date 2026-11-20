//! Handlers for calls the generated table does not expand inline.
//!
//! Every handler runs under the graphics lock, on behalf of one process. It
//! resolves handles through the process's table, validates every value, copies
//! what LVGL keeps, and writes the result back through the transport.

use super::{
    Call, Function, Word,
    error::{Error, Result},
    marshal::{
        ButtonMap, MAX_STRING_BYTES, MAX_WIDGET_OPTIONS_BYTES, read_c_string, read_textarea_string,
        read_utf8_c_string, read_widget_options,
    },
    state::{Kind, State},
    transport::Transport,
};
use crate::{Color, Window, lvgl};
use alloc::vec::Vec;
use core::{ffi::CStr, ptr::NonNull};

const MAX_RANGE_VALUE: i32 = 1_000_000;

// Checked narrowing: a no-op on 32-bit callers, a range check on 64-bit ones.
fn checked_u32(value: Word) -> Result<u32> {
    u32::try_from(value).map_err(|_| Error::InvalidValue)
}

/// A signed 32-bit argument. A 32-bit caller passes it zero-extended in its
/// word and a 64-bit caller sign-extended; both are the same `i32`. On a
/// 32-bit host every word is one.
pub(super) fn signed_word(value: Word) -> Result<i32> {
    let narrowed = value as u32 as i32;
    if value == narrowed as isize as Word || value == narrowed as u32 as Word {
        Ok(narrowed)
    } else {
        Err(Error::InvalidValue)
    }
}

pub(super) fn dispatch_grid_tracks(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [object, columns, rows, ..] = call.arguments;
    state.object(object, None)?;
    let tracks = super::marshal::GridTracks::decode(
        memory,
        columns,
        rows,
        lvgl::LV_GRID_TEMPLATE_LAST as i32,
        lvgl::LV_GRID_CONTENT as i32,
    )?;
    state.set_grid_tracks(object, tracks)
}

fn valid_calendar_date(year: u32, month: u32, day: u32) -> bool {
    if year == 0 || year > u16::MAX as u32 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days_in_month).contains(&day)
}

pub(super) fn dispatch_calendar_set_today(
    state: &mut State,
    _memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [calendar, year, month, day, ..] = call.arguments;
    let year = checked_u32(year)?;
    let month = checked_u32(month)?;
    let day = checked_u32(day)?;
    if !valid_calendar_date(year, month, day) {
        return Err(Error::InvalidValue);
    }
    let calendar = state.object(calendar, Some(Kind::Calendar))?;
    unsafe { lvgl::lv_calendar_set_today_date(calendar, year, month, day) };
    Ok(())
}

pub(super) fn dispatch_calendar_set_highlights(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, pointer, count, ..] = call.arguments;
    if count > 32 {
        return Err(Error::ResourceLimit);
    }
    let byte_count = count.checked_mul(4).ok_or(Error::InvalidMemory)?;
    // Packed records: year (u16), month (u8), day (u8).
    let records = memory.bytes(pointer, byte_count)?;
    let mut dates = Vec::with_capacity(count);
    for record in records.chunks_exact(4) {
        let year = u16::from_le_bytes([record[0], record[1]]);
        let (month, day) = (record[2], record[3]);
        if !valid_calendar_date(u32::from(year), u32::from(month), u32::from(day)) {
            return Err(Error::InvalidValue);
        }
        dates.push(lvgl::lv_calendar_date_t { year, month, day });
    }
    let calendar = state.object(handle, Some(Kind::Calendar))?;
    let (dates_pointer, old_dates) = state.replace_calendar_highlights(handle, dates)?;
    unsafe { lvgl::lv_calendar_set_highlighted_dates(calendar, dates_pointer, count) };
    drop(old_dates);
    Ok(())
}

pub(super) fn dispatch_calendar_get_shown_date(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, output, ..] = call.arguments;
    memory.validate(output, 4)?;
    let calendar = state.object(handle, Some(Kind::Calendar))?;
    let date = unsafe { lvgl::lv_calendar_get_showed_date(calendar) };
    if date.is_null() {
        return Err(Error::NativeFailure);
    }
    let date = unsafe { *date };
    memory.put::<u16>(output, date.year);
    memory.put::<u8>(output + 2, date.month);
    memory.put::<u8>(output + 3, date.day);
    Ok(())
}

pub(super) fn dispatch_chart_add_series(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, rgb, ..] = call.arguments;
    if state.chart_series_count(handle)? >= 8 {
        return Err(Error::ResourceLimit);
    }
    let chart = state.object(handle, Some(Kind::Chart))?;
    let rgb = checked_u32(rgb)?;
    let color = unsafe { lvgl::lv_color_hex(rgb) };
    let series = unsafe {
        lvgl::lv_chart_add_series(chart, color, lvgl::lv_chart_axis_t_LV_CHART_AXIS_PRIMARY_Y)
    };
    let series = NonNull::new(series).ok_or(Error::NativeFailure)?;
    let slot = state.add_chart_series(handle, series)?;
    memory.put::<u32>(call.result, slot);
    Ok(())
}

pub(super) fn dispatch_chart_set_values(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, slot, pointer, count, ..] = call.arguments;
    if count == 0 || count > 128 {
        return Err(Error::ResourceLimit);
    }
    let chart = state.object(handle, Some(Kind::Chart))?;
    let point_count = unsafe { lvgl::lv_chart_get_point_count(chart) } as usize;
    if count > point_count {
        return Err(Error::InvalidValue);
    }
    let series = state.chart_series(handle, slot)?;
    let values = memory.values::<i32>(pointer, count)?;
    for (index, value) in values.enumerate() {
        let index = u32::try_from(index).map_err(|_| Error::InvalidValue)?;
        unsafe { lvgl::lv_chart_set_series_value_by_id(chart, series.as_ptr(), index, value) };
    }
    Ok(())
}

pub(super) fn dispatch_chart_get_value(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, slot, index, ..] = call.arguments;
    let chart = state.object(handle, Some(Kind::Chart))?;
    let point_count = unsafe { lvgl::lv_chart_get_point_count(chart) };
    let index = checked_u32(index)?;
    if index >= point_count {
        return Err(Error::InvalidValue);
    }
    let series = state.chart_series(handle, slot)?;
    let values = unsafe { lvgl::lv_chart_get_series_y_array(chart, series.as_ptr()) };
    if values.is_null() {
        return Err(Error::NativeFailure);
    }
    let value = unsafe { *values.add(index as usize) };
    memory.put::<i32>(call.result, value);
    Ok(())
}

pub(super) fn dispatch_line_set_points(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, pointer, count, ..] = call.arguments;
    if count > 128 {
        return Err(Error::ResourceLimit);
    }
    let byte_count = count.checked_mul(8).ok_or(Error::InvalidMemory)?;
    // Packed records: x (i32), y (i32).
    let records = memory.bytes(pointer, byte_count)?;
    let mut points = Vec::with_capacity(count);
    for record in records.chunks_exact(8) {
        points.push(lvgl::lv_point_precise_t {
            x: i32::from_le_bytes([record[0], record[1], record[2], record[3]]),
            y: i32::from_le_bytes([record[4], record[5], record[6], record[7]]),
        });
    }
    let line = state.object(handle, Some(Kind::Line))?;
    let (points_pointer, old_points) = state.replace_line_points(handle, points)?;
    unsafe { lvgl::lv_line_set_points(line, points_pointer, count as u32) };
    drop(old_points);
    Ok(())
}

/// Append a text label or a button to a list: one string copied by LVGL,
/// charged to the process's widget budget, registered under its own kind.
fn list_add(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
    button: bool,
) -> Result<()> {
    let [list_handle, text_pointer, ..] = call.arguments;
    let list = state.object(list_handle, Some(Kind::List))?;
    if unsafe { lvgl::lv_obj_get_child_count(list) } >= 64 {
        return Err(Error::ResourceLimit);
    }
    let text = read_c_string(memory, text_pointer)?;
    text.to_str().map_err(|_| Error::InvalidValue)?;
    let text_bytes = text.to_bytes_with_nul().len();
    state.validate_new_widget_options_size(text_bytes)?;
    state.ensure_capacity()?;
    let (pointer, kind) = unsafe {
        if button {
            (
                lvgl::lv_list_add_button(list, core::ptr::null(), text.as_ptr()),
                Kind::ListButton,
            )
        } else {
            (lvgl::lv_list_add_text(list, text.as_ptr()), Kind::ListText)
        }
    };
    let handle = state.insert(pointer, kind, Some(list_handle))?;
    state.commit_widget_options_size(handle, text_bytes)?;
    memory.put_word(call.result, handle);
    Ok(())
}

pub(super) fn dispatch_list_add_text(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    list_add(state, memory, call, false)
}

pub(super) fn dispatch_list_add_text_button(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    list_add(state, memory, call, true)
}

pub(super) fn dispatch_list_set_button_text(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [list_handle, button_handle, text_pointer, ..] = call.arguments;
    let list = state.object(list_handle, Some(Kind::List))?;
    let button = state.object(button_handle, Some(Kind::ListButton))?;
    if unsafe { lvgl::lv_obj_get_parent(button) } != list {
        return Err(Error::InvalidHandle);
    }
    let text = read_c_string(memory, text_pointer)?;
    text.to_str().map_err(|_| Error::InvalidValue)?;
    let text_bytes = text.to_bytes_with_nul().len();
    state.validate_widget_options_size(button_handle, text_bytes)?;
    unsafe { lvgl::lv_list_set_button_text(list, button, text.as_ptr()) };
    state.commit_widget_options_size(button_handle, text_bytes)
}

pub(super) fn dispatch_tileview_add_tile(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [view_handle, column, row, directions, ..] = call.arguments;
    let view = state.object(view_handle, Some(Kind::Tileview))?;
    let column = u8::try_from(column).map_err(|_| Error::InvalidValue)?;
    let row = u8::try_from(row).map_err(|_| Error::InvalidValue)?;
    let directions = checked_u32(directions)?;
    if directions & !lvgl::lv_dir_t_LV_DIR_ALL != 0 {
        return Err(Error::InvalidValue);
    }
    if unsafe { lvgl::lv_obj_get_child_count(view) } >= 32 {
        return Err(Error::ResourceLimit);
    }
    if state.tile_position_in_use(view_handle, column, row)? {
        return Err(Error::InvalidValue);
    }
    state.ensure_capacity()?;
    let pointer = unsafe { lvgl::lv_tileview_add_tile(view, column, row, directions) };
    let handle = state.insert(pointer, Kind::Tile, Some(view_handle))?;
    state.set_tile_position(handle, (column, row))?;
    memory.put_word(call.result, handle);
    Ok(())
}

pub(super) fn dispatch_tileview_set_tile(
    state: &mut State,
    _memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [view_handle, tile_handle, animated, ..] = call.arguments;
    if animated > 1 {
        return Err(Error::InvalidValue);
    }
    if !state.is_child_of(tile_handle, view_handle, Kind::Tile)? {
        return Err(Error::InvalidHandle);
    }
    let view = state.object(view_handle, Some(Kind::Tileview))?;
    let tile = state.object(tile_handle, Some(Kind::Tile))?;
    unsafe { lvgl::lv_tileview_set_tile(view, tile, animated != 0) };
    Ok(())
}

pub(super) fn dispatch_tileview_get_tile_active(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [view_handle, ..] = call.arguments;
    let view = state.object(view_handle, Some(Kind::Tileview))?;
    let active = unsafe { lvgl::lv_tileview_get_tile_active(view) };
    let handle = state.handle_for(active);
    if !active.is_null() && handle == 0 {
        return Err(Error::InvalidHandle);
    }
    memory.put_word(call.result, handle);
    Ok(())
}

pub(super) fn dispatch_theme_variant(
    state: &mut State,
    _memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, variant, ..] = call.arguments;
    let variant = checked_u32(variant)?;
    if variant > lvgl::lv_theme_shadcn_variant_t_LV_THEME_SHADCN_VARIANT_DESTRUCTIVE {
        return Err(Error::InvalidValue);
    }
    let (object, kind) = state.object_with_kind(handle)?;
    if !matches!(kind, Kind::Button | Kind::Badge) {
        return Err(Error::InvalidHandle);
    }
    unsafe { lvgl::lv_theme_shadcn_set_variant(object, variant) };
    Ok(())
}

pub(super) fn dispatch_theme_error(
    state: &mut State,
    _memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [handle, error, ..] = call.arguments;
    if error > 1 {
        return Err(Error::InvalidValue);
    }
    let (object, kind) = state.object_with_kind(handle)?;
    if !matches!(
        kind,
        Kind::Textarea | Kind::Spinbox | Kind::Dropdown | Kind::Roller
    ) {
        return Err(Error::InvalidHandle);
    }
    unsafe { lvgl::lv_theme_shadcn_set_error(object, error != 0) };
    Ok(())
}

pub(super) fn dispatch_badge_create(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [parent_handle, text_pointer, variant, ..] = call.arguments;
    let variant = checked_u32(variant)?;
    if variant > lvgl::lv_theme_shadcn_variant_t_LV_THEME_SHADCN_VARIANT_DESTRUCTIVE {
        return Err(Error::InvalidValue);
    }
    let parent = state.object(parent_handle, None)?;
    let text = read_c_string(memory, text_pointer)?;
    text.to_str().map_err(|_| Error::InvalidValue)?;
    let text_bytes = text.to_bytes_with_nul().len();
    state.validate_new_widget_options_size(text_bytes)?;
    state.ensure_capacity()?;
    let badge = unsafe { lvgl::lv_label_create(parent) };
    let handle = state.insert(badge, Kind::Badge, Some(parent_handle))?;
    unsafe {
        lvgl::lv_label_set_text(badge, text.as_ptr());
        lvgl::lv_theme_shadcn_set_variant(badge, variant);
    }
    state.commit_widget_options_size(handle, text_bytes)?;
    memory.put_word(call.result, handle);
    Ok(())
}

pub(super) fn dispatch_card_create(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let [parent_handle, ..] = call.arguments;
    let parent = state.object(parent_handle, None)?;
    state.ensure_capacity()?;
    let card = unsafe { lvgl::lv_obj_create(parent) };
    let handle = state.insert(card, Kind::Card, Some(parent_handle))?;
    unsafe { crate::theme::set_surface(card, crate::theme::Surface::Card) };
    memory.put_word(call.result, handle);
    Ok(())
}

fn copy_selected_option(
    memory: &mut dyn Transport,
    options: *const core::ffi::c_char,
    selected: u32,
    output: Word,
    capacity: Word,
    result: Word,
) -> Result<i32> {
    if options.is_null() {
        return Err(Error::NativeFailure);
    }
    let options = unsafe { CStr::from_ptr(options) }.to_bytes();
    if options.len() >= MAX_WIDGET_OPTIONS_BYTES {
        return Err(Error::ResourceLimit);
    }
    let option = options
        .split(|byte| *byte == b'\n')
        .nth(selected as usize)
        .ok_or(Error::InvalidValue)?;
    let required = option.len() + 1;
    if required > MAX_STRING_BYTES {
        return Err(Error::ResourceLimit);
    }
    if capacity < required {
        return Err(Error::InvalidValue);
    }
    let output_end = output
        .checked_add(required as Word)
        .ok_or(Error::InvalidMemory)?;
    let result_end = result
        .checked_add(core::mem::size_of::<i32>() as Word)
        .ok_or(Error::InvalidMemory)?;
    if output < result_end && result < output_end {
        return Err(Error::InvalidMemory);
    }
    let destination = memory.bytes_mut(output, required)?;
    destination[..option.len()].copy_from_slice(option);
    destination[option.len()] = 0;
    i32::try_from(option.len()).map_err(|_| Error::ResourceLimit)
}

type Object = *mut lvgl::lv_obj_t;

// Calls on one object with scalar words. The table generated from the ABI
// declaration passes the native function; each shape is one function here, so
// a dozen calls cost a dozen call sites rather than a dozen copies of the
// handle lookup and the range check. The object is resolved first, so a bad
// handle outranks a bad value.

pub(super) fn object_call(
    state: &mut State,
    call: &Call,
    kind: Option<Kind>,
    native: fn(Object),
) -> Result<()> {
    native(state.object(call.arguments[0], kind)?);
    Ok(())
}

pub(super) fn object_set_bool(
    state: &mut State,
    call: &Call,
    kind: Option<Kind>,
    native: fn(Object, bool),
) -> Result<()> {
    let object = state.object(call.arguments[0], kind)?;
    if call.arguments[1] > 1 {
        return Err(Error::InvalidValue);
    }
    native(object, call.arguments[1] != 0);
    Ok(())
}

pub(super) fn object_set_u8(
    state: &mut State,
    call: &Call,
    kind: Option<Kind>,
    native: fn(Object, u8),
) -> Result<()> {
    let object = state.object(call.arguments[0], kind)?;
    native(
        object,
        u8::try_from(call.arguments[1]).map_err(|_| Error::InvalidValue)?,
    );
    Ok(())
}

pub(super) fn object_set_u32(
    state: &mut State,
    call: &Call,
    kind: Option<Kind>,
    range: core::ops::RangeInclusive<u32>,
    native: fn(Object, u32),
) -> Result<()> {
    let object = state.object(call.arguments[0], kind)?;
    let value = checked_u32(call.arguments[1])?;
    if !range.contains(&value) {
        return Err(Error::InvalidValue);
    }
    native(object, value);
    Ok(())
}

/// A `u32` made of the bits in `mask` only.
pub(super) fn object_set_mask(
    state: &mut State,
    call: &Call,
    kind: Option<Kind>,
    mask: u32,
    native: fn(Object, u32),
) -> Result<()> {
    let object = state.object(call.arguments[0], kind)?;
    let value = checked_u32(call.arguments[1])?;
    if value & !mask != 0 {
        return Err(Error::InvalidValue);
    }
    native(object, value);
    Ok(())
}

pub(super) fn object_set_i32(
    state: &mut State,
    call: &Call,
    kind: Option<Kind>,
    native: fn(Object, i32),
) -> Result<()> {
    let object = state.object(call.arguments[0], kind)?;
    native(object, signed_word(call.arguments[1])?);
    Ok(())
}

pub(super) fn object_set_i32_pair(
    state: &mut State,
    call: &Call,
    native: fn(Object, i32, i32),
) -> Result<()> {
    let object = state.object(call.arguments[0], None)?;
    let first = signed_word(call.arguments[1])?;
    native(object, first, signed_word(call.arguments[2])?);
    Ok(())
}

pub(super) fn object_get_u32(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
    kind: Option<Kind>,
    native: fn(Object) -> u32,
) -> Result<()> {
    let value = native(state.object(call.arguments[0], kind)?);
    memory.put::<u32>(call.result, value);
    Ok(())
}

pub(super) fn object_get_i32(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
    kind: Option<Kind>,
    native: fn(Object) -> i32,
) -> Result<()> {
    let value = native(state.object(call.arguments[0], kind)?);
    memory.put::<i32>(call.result, value);
    Ok(())
}

/// The parent-less constructor: windows belong to the manager, not to an
/// object tree of the caller.
pub(super) fn dispatch_window_create(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    state.ensure_capacity()?;
    let manager = state.manager.ok_or(Error::Unavailable)?;
    let window = task::block_on(manager.create_window()).map_err(|_| Error::NativeFailure)?;
    let window: NonNull<Window> = window.into();
    let handle = state.insert(window.as_ptr().cast(), Kind::Window, None)?;
    memory.put_word(call.result, handle);
    Ok(())
}

/// A `(parent) -> object` constructor, registered under `kind`.
pub(super) fn create(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
    kind: Kind,
    constructor: unsafe extern "C" fn(*mut lvgl::lv_obj_t) -> *mut lvgl::lv_obj_t,
) -> Result<()> {
    state.ensure_capacity()?;
    let parent_handle = call.arguments[0];
    let parent = state.object(parent_handle, None)?;
    let object = unsafe { constructor(parent) };
    let handle = state.insert(object, kind, Some(parent_handle))?;
    memory.put_word(call.result, handle);
    Ok(())
}

/// Calls without a generated arm. Each family shares its validation.
#[allow(clippy::useless_conversion)]
pub(super) fn manual(
    state: &mut State,
    memory: &mut dyn Transport,
    call: &Call,
    function: Function,
) -> Result<()> {
    let [a, b, c, ..] = call.arguments;
    match function {
        Function::FilePickerCreate => {
            state.ensure_capacity()?;
            let parent = state.object(a, None)?;
            let mode = match b {
                0 => crate::FilePickerMode::OpenFile,
                1 => crate::FilePickerMode::SaveFile,
                2 => crate::FilePickerMode::Directory,
                _ => return Err(Error::InvalidValue),
            };
            let initial = read_utf8_c_string(memory, c)?;
            let (vfs, task) = state.file_system.ok_or(Error::Unavailable)?;
            let picker = unsafe { crate::FilePicker::new(parent, vfs, task, mode, initial) }
                .map_err(|error| match error {
                    crate::Error::InvalidParameter => Error::InvalidValue,
                    _ => Error::NativeFailure,
                })?;
            let handle = state.insert(picker.as_ptr().cast(), Kind::FilePicker, Some(a))?;
            memory.put_word(call.result, handle);
        }
        Function::FilePickerReset => {
            let picker = state
                .object(a, Some(Kind::FilePicker))?
                .cast::<crate::FilePicker>();
            let mode = match b {
                0 => crate::FilePickerMode::OpenFile,
                1 => crate::FilePickerMode::SaveFile,
                2 => crate::FilePickerMode::Directory,
                _ => return Err(Error::InvalidValue),
            };
            let initial = read_utf8_c_string(memory, c)?;
            unsafe { (*picker).reset(mode, initial) }.map_err(|error| match error {
                crate::Error::InvalidParameter => Error::InvalidValue,
                _ => Error::NativeFailure,
            })?;
        }
        Function::FilePickerGetStatus => {
            let picker = state
                .object(a, Some(Kind::FilePicker))?
                .cast::<crate::FilePicker>();
            let status = match unsafe { (*picker).status() } {
                crate::FilePickerStatus::Pending => 0u32,
                crate::FilePickerStatus::Selected => 1,
                crate::FilePickerStatus::Cancelled => 2,
            };
            memory.put::<u32>(call.result, status);
        }
        Function::FilePickerGetPath => {
            let picker = state
                .object(a, Some(Kind::FilePicker))?
                .cast::<crate::FilePicker>();
            let path = unsafe { (*picker).selected_path() };
            let length = if let Some(path) = path {
                if path.len() as Word >= c {
                    return Err(Error::InvalidValue);
                }
                // Validate only the range written. Retain the selection on every
                // error so a caller can retry with a valid/larger buffer.
                let bytes = memory.bytes_mut(b, path.len() + 1)?;
                bytes[..path.len()].copy_from_slice(path.as_bytes());
                bytes[path.len()] = 0;
                path.len() as i32
            } else {
                -1
            };
            memory.put::<i32>(call.result, length);
        }
        Function::ObjectSetPadding => {
            let object = state.object(a, None)?;
            let padding = i32::try_from(b).map_err(|_| Error::InvalidValue)?;
            if !(0..=64).contains(&padding) {
                return Err(Error::InvalidValue);
            }
            unsafe {
                lvgl::lv_obj_set_style_pad_all(object, padding, 0);
                lvgl::lv_obj_set_style_pad_row(object, padding, 0);
                lvgl::lv_obj_set_style_pad_column(object, padding, 0);
            }
        }
        Function::ObjectAlign => {
            let object = state.object(a, None)?;
            if b > lvgl::lv_align_t_LV_ALIGN_OUT_RIGHT_BOTTOM as Word {
                return Err(Error::InvalidValue);
            }
            let x = signed_word(c)?;
            let y = signed_word(call.arguments[3])?;
            unsafe { lvgl::lv_obj_align(object, b as _, x, y) };
        }
        Function::ObjectSetBackground => {
            let object = state.object(a, None)?;
            let rgb = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let opacity = u8::try_from(c).map_err(|_| Error::InvalidValue)?;
            let selector = u32::try_from(call.arguments[3]).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if rgb > 0x00ff_ffff {
                return Err(Error::InvalidValue);
            }
            let color = unsafe { lvgl::lv_color_hex(rgb) };
            unsafe {
                lvgl::lv_obj_set_style_bg_color(object, color, selector);
                lvgl::lv_obj_set_style_bg_opa(object, opacity, selector);
            }
        }
        Function::ObjectSetTextColor => {
            let object = state.object(a, None)?;
            let rgb = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let selector = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if rgb > 0x00ff_ffff {
                return Err(Error::InvalidValue);
            }
            let color = unsafe { lvgl::lv_color_hex(rgb) };
            unsafe { lvgl::lv_obj_set_style_text_color(object, color, selector) };
        }
        Function::ObjectSetBorder => {
            let object = state.object(a, None)?;
            let rgb = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let width = signed_word(c)?;
            let side = u32::try_from(call.arguments[3]).map_err(|_| Error::InvalidValue)?;
            let selector = u32::try_from(call.arguments[4]).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if rgb > 0x00ff_ffff || !(0..=64).contains(&width) || side & !0x1f != 0 {
                return Err(Error::InvalidValue);
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
            let selector = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if !(0..=1024).contains(&radius) {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_style_radius(object, radius, selector) };
        }
        Function::ObjectSetTextAlign => {
            let object = state.object(a, None)?;
            let align = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let selector = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if align > lvgl::lv_text_align_t_LV_TEXT_ALIGN_RIGHT {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_style_text_align(object, align, selector) };
        }
        Function::ObjectSetFont => {
            let object = state.object(a, None)?;
            let font = match b {
                0 => crate::fonts::get_font_small(),
                1 => crate::fonts::get_font_medium(),
                2 => crate::fonts::get_font_large(),
                3 => crate::fonts::get_font_extra_large(),
                4 => crate::fonts::get_font_monospace_small(),
                5 => crate::fonts::get_font_monospace_medium(),
                _ => return Err(Error::InvalidValue),
            };
            let selector = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            unsafe { lvgl::lv_obj_set_style_text_font(object, font, selector) };
        }
        Function::ObjectSetSurface => {
            let object = state.object(a, None)?;
            let surface = match b {
                0 => crate::theme::Surface::Background,
                1 => crate::theme::Surface::Card,
                2 => crate::theme::Surface::Popover,
                3 => crate::theme::Surface::Muted,
                _ => return Err(Error::InvalidValue),
            };
            unsafe { crate::theme::set_surface(object, surface) };
        }
        Function::ObjectSetMinMaxSize => {
            let object = state.object(a, None)?;
            let min_width = signed_word(b)?;
            let max_width = signed_word(c)?;
            let min_height = signed_word(call.arguments[3])?;
            let max_height = signed_word(call.arguments[4])?;
            let selector = u32::try_from(call.arguments[5]).map_err(|_| Error::InvalidValue)?;
            super::marshal::validate_style_selector(selector)?;
            if !(0..=16_384).contains(&min_width)
                || !(0..=16_384).contains(&max_width)
                || !(0..=16_384).contains(&min_height)
                || !(0..=16_384).contains(&max_height)
                || min_width > max_width
                || min_height > max_height
            {
                return Err(Error::InvalidValue);
            }
            unsafe {
                lvgl::lv_obj_set_style_min_width(object, min_width, selector);
                lvgl::lv_obj_set_style_max_width(object, max_width, selector);
                lvgl::lv_obj_set_style_min_height(object, min_height, selector);
                lvgl::lv_obj_set_style_max_height(object, max_height, selector);
            }
        }
        Function::CalendarSetMonthShown => {
            let calendar = state.object(a, Some(Kind::Calendar))?;
            let year = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let month = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            if year == 0 || year > u16::MAX as u32 || !(1..=12).contains(&month) {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_calendar_set_month_shown(calendar, year, month) };
        }
        Function::ChartSetType => {
            let chart = state.object(a, Some(Kind::Chart))?;
            let chart_type = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            if chart_type > lvgl::lv_chart_type_t_LV_CHART_TYPE_SCATTER {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_chart_set_type(chart, chart_type) };
        }
        Function::ChartSetAxisRange => {
            let chart = state.object(a, Some(Kind::Chart))?;
            let axis = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let minimum = signed_word(c)?;
            let maximum = signed_word(call.arguments[3])?;
            if !matches!(axis, 0 | 1 | 2 | 4)
                || minimum >= maximum
                || minimum < -MAX_RANGE_VALUE
                || maximum > MAX_RANGE_VALUE
            {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_chart_set_axis_range(chart, axis, minimum, maximum) };
        }
        Function::SpinboxSetRange => {
            let spinbox = state.object(a, Some(Kind::Spinbox))?;
            let minimum = signed_word(b)?;
            let maximum = signed_word(c)?;
            if minimum >= maximum || minimum < -MAX_RANGE_VALUE || maximum > MAX_RANGE_VALUE {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_spinbox_set_range(spinbox, minimum, maximum) };
        }
        Function::SpinboxSetValue => {
            let spinbox = state.object(a, Some(Kind::Spinbox))?;
            let value = signed_word(b)?;
            let minimum = unsafe { lvgl::lv_spinbox_get_min_value(spinbox) };
            let maximum = unsafe { lvgl::lv_spinbox_get_max_value(spinbox) };
            if !(minimum..=maximum).contains(&value) {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_spinbox_set_value(spinbox, value) };
        }
        Function::SpinboxSetDigitFormat => {
            let spinbox = state.object(a, Some(Kind::Spinbox))?;
            let digits = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let separator = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            if !(1..=10).contains(&digits) || separator >= digits {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_spinbox_set_digit_format(spinbox, digits, separator) };
        }
        Function::RangeSetRange => {
            let (object, kind) = state.object_with_kind(a)?;
            let minimum = signed_word(b)?;
            let maximum = signed_word(c)?;
            if minimum >= maximum || minimum < -MAX_RANGE_VALUE || maximum > MAX_RANGE_VALUE {
                return Err(Error::InvalidValue);
            }
            match kind {
                Kind::Arc => unsafe { lvgl::lv_arc_set_range(object, minimum, maximum) },
                Kind::Bar => unsafe { lvgl::lv_bar_set_range(object, minimum, maximum) },
                Kind::Slider => unsafe { lvgl::lv_slider_set_range(object, minimum, maximum) },
                _ => return Err(Error::InvalidHandle),
            }
        }
        Function::RangeSetValue => {
            let (object, kind) = state.object_with_kind(a)?;
            let value = signed_word(b)?;
            if !(-MAX_RANGE_VALUE..=MAX_RANGE_VALUE).contains(&value) || c > 1 {
                return Err(Error::InvalidValue);
            }
            match kind {
                Kind::Arc => {
                    if c != 0 {
                        return Err(Error::InvalidValue);
                    }
                    let minimum = unsafe { lvgl::lv_arc_get_min_value(object) };
                    let maximum = unsafe { lvgl::lv_arc_get_max_value(object) };
                    if !(minimum..=maximum).contains(&value) {
                        return Err(Error::InvalidValue);
                    }
                    unsafe { lvgl::lv_arc_set_value(object, value) };
                }
                Kind::Bar => {
                    let minimum = unsafe { lvgl::lv_bar_get_min_value(object) };
                    let maximum = unsafe { lvgl::lv_bar_get_max_value(object) };
                    if !(minimum..=maximum).contains(&value) {
                        return Err(Error::InvalidValue);
                    }
                    unsafe { lvgl::lv_bar_set_value(object, value, c != 0) };
                }
                Kind::Slider => {
                    let minimum = unsafe { lvgl::lv_slider_get_min_value(object) };
                    let maximum = unsafe { lvgl::lv_slider_get_max_value(object) };
                    if !(minimum..=maximum).contains(&value) {
                        return Err(Error::InvalidValue);
                    }
                    unsafe { lvgl::lv_slider_set_value(object, value, c != 0) };
                }
                _ => return Err(Error::InvalidHandle),
            }
        }
        Function::RangeGetValue => {
            let (object, kind) = state.object_with_kind(a)?;
            let value = match kind {
                Kind::Arc => unsafe { lvgl::lv_arc_get_value(object) },
                Kind::Bar => unsafe { lvgl::lv_bar_get_value(object) },
                Kind::Slider => unsafe { lvgl::lv_slider_get_value(object) },
                _ => return Err(Error::InvalidHandle),
            };
            memory.put::<i32>(call.result, value);
        }
        Function::RangeGetStartValue => {
            let (object, kind) = state.object_with_kind(a)?;
            let value = match kind {
                Kind::Bar => unsafe { lvgl::lv_bar_get_start_value(object) },
                Kind::Slider => unsafe { lvgl::lv_slider_get_left_value(object) },
                _ => return Err(Error::InvalidHandle),
            };
            memory.put::<i32>(call.result, value);
        }
        Function::RangeSetMode => {
            let (object, kind) = state.object_with_kind(a)?;
            if b > 2 {
                return Err(Error::InvalidValue);
            }
            match kind {
                Kind::Arc => unsafe { lvgl::lv_arc_set_mode(object, b as _) },
                Kind::Bar => unsafe { lvgl::lv_bar_set_mode(object, b as _) },
                Kind::Slider => unsafe { lvgl::lv_slider_set_mode(object, b as _) },
                _ => return Err(Error::InvalidHandle),
            }
        }
        Function::RangeSetOrientation => {
            let (object, kind) = state.object_with_kind(a)?;
            if b > 2 {
                return Err(Error::InvalidValue);
            }
            match kind {
                Kind::Bar => unsafe { lvgl::lv_bar_set_orientation(object, b as _) },
                Kind::Slider => unsafe { lvgl::lv_slider_set_orientation(object, b as _) },
                Kind::Switch => unsafe { lvgl::lv_switch_set_orientation(object, b as _) },
                _ => return Err(Error::InvalidHandle),
            }
        }
        Function::SpinnerSetAnimation => {
            let spinner = state.object(a, Some(Kind::Spinner))?;
            let duration = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let sweep = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            if duration > 600_000 || sweep > 360 {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_spinner_set_anim_params(spinner, duration, sweep) };
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
            let selected = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let count = unsafe { lvgl::lv_dropdown_get_option_count(dropdown) };
            if selected >= count {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_dropdown_set_selected(dropdown, selected) };
        }
        Function::RollerSetOptions => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let options = read_widget_options(memory, b)?;
            if c > 1 {
                return Err(Error::InvalidValue);
            }
            let bytes = options.to_bytes_with_nul().len();
            if c == lvgl::lv_roller_mode_t_LV_ROLLER_MODE_INFINITE as Word
                && bytes > super::marshal::MAX_ROLLER_INFINITE_OPTIONS_BYTES
            {
                return Err(Error::ResourceLimit);
            }
            let retained_bytes = bytes
                .checked_mul(
                    if c == lvgl::lv_roller_mode_t_LV_ROLLER_MODE_INFINITE as Word {
                        15
                    } else {
                        1
                    },
                )
                .ok_or(Error::ResourceLimit)?;
            state.validate_widget_options_size(a, retained_bytes)?;
            // LVGL copies the option string, including in infinite mode.
            unsafe { lvgl::lv_roller_set_options(roller, options.as_ptr(), c as _) };
            state.commit_widget_options_size(a, retained_bytes)?;
        }
        Function::RollerSetSelected => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let selected = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let animated = call.arguments[2];
            if animated > 1 || selected >= unsafe { lvgl::lv_roller_get_option_count(roller) } {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_roller_set_selected(roller, selected, animated != 0) };
        }
        Function::DropdownGetSelectedString | Function::RollerGetSelectedString => {
            let (selected, options) = if function == Function::DropdownGetSelectedString {
                let dropdown = state.object(a, Some(Kind::Dropdown))?;
                unsafe {
                    (
                        lvgl::lv_dropdown_get_selected(dropdown),
                        lvgl::lv_dropdown_get_options(dropdown),
                    )
                }
            } else {
                let roller = state.object(a, Some(Kind::Roller))?;
                unsafe {
                    (
                        lvgl::lv_roller_get_selected(roller),
                        lvgl::lv_roller_get_options(roller),
                    )
                }
            };
            let capacity = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            if capacity as usize > MAX_STRING_BYTES {
                return Err(Error::ResourceLimit);
            }
            let length =
                copy_selected_option(memory, options, selected, b, capacity as Word, call.result)?;
            memory.put::<i32>(call.result, length);
        }
        Function::ObjectSetFlexAlign => {
            let object = state.object(a, None)?;
            let main = b;
            let cross = c;
            let track = call.arguments[3];
            if [main, cross, track]
                .iter()
                .any(|&align| align > lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN as Word)
            {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_flex_align(object, main as _, cross as _, track as _) };
        }
        Function::GridFr => {
            let weight = u8::try_from(a).map_err(|_| Error::InvalidValue)?;
            if !(0..=99).contains(&weight) {
                return Err(Error::InvalidValue);
            }
            let descriptor = unsafe { lvgl::lv_grid_fr(weight) };
            memory.put::<i32>(call.result, descriptor);
        }
        Function::ObjectSetGridAlign => {
            let object = state.object(a, None)?;
            let column = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let row = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            if column > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
                || row > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
            {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_grid_align(object, column, row) };
        }
        Function::ObjectSetGridCell => {
            let object = state.object(a, None)?;
            let column_align = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            let column = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            let column_span = u32::try_from(call.arguments[3]).map_err(|_| Error::InvalidValue)?;
            let row_align = u32::try_from(call.arguments[4]).map_err(|_| Error::InvalidValue)?;
            let row = u32::try_from(call.arguments[5]).map_err(|_| Error::InvalidValue)?;
            let row_span = u32::try_from(call.arguments[6]).map_err(|_| Error::InvalidValue)?;
            if column_align > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
                || row_align > lvgl::lv_grid_align_t_LV_GRID_ALIGN_SPACE_BETWEEN
                || column >= (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
                || row >= (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
                || column_span == 0
                || column_span > (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
                || row_span == 0
                || row_span > (super::marshal::MAX_GRID_TRACKS_PER_AXIS - 1) as u32
            {
                return Err(Error::InvalidValue);
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
        Function::TabviewAddTab => {
            state.ensure_capacity()?;
            let parent = state.object(a, Some(Kind::TabView))?;
            let text = read_c_string(memory, b)?;
            // LVGL copies the tab caption; it never retains guest memory.
            let tab = unsafe { lvgl::lv_tabview_add_tab(parent, text.as_ptr()) };
            let handle = state.insert(tab, Kind::Object, Some(a))?;
            memory.put_word(call.result, handle);
        }
        Function::ObjectDelete => {
            unsafe { lvgl::lv_obj_delete(state.object(a, None)?) };
        }
        Function::WindowSetIcon => {
            let pointer = state.object(a, Some(Kind::Window))?;
            let text = read_c_string(memory, b)?
                .to_str()
                .map_err(|_| Error::InvalidValue)?;
            let color = u32::try_from(c).map_err(|_| Error::InvalidValue)?;
            let color = lvgl::lv_color_t {
                red: (color >> 16) as u8,
                green: (color >> 8) as u8,
                blue: color as u8,
            };
            let mut window = unsafe { Window::from_raw(pointer) }.ok_or(Error::InvalidHandle)?;
            unsafe {
                window
                    .as_mut()
                    .set_icon(text, Color::from_lvgl_color(color))
            };
        }
        Function::WindowPopEvent | Function::WindowPopPointerEvent => {
            let pointer = state.object(a, Some(Kind::Window))?;
            memory.validate(b, size_of::<u32>())?;
            memory.validate(c, memory.word_size())?;
            if function == Function::WindowPopPointerEvent {
                for &address in &call.arguments[3..7] {
                    memory.validate(address, size_of::<i32>())?;
                }
            }
            let mut window = unsafe { Window::from_raw(pointer) }.ok_or(Error::InvalidHandle)?;
            let event = unsafe { window.as_mut().pop_event() };
            if function == Function::WindowPopPointerEvent {
                let point = event.as_ref().and_then(|event| event.pointer).unwrap_or([
                    i32::MIN,
                    i32::MIN,
                    0,
                    0,
                ]);
                for (&offset, value) in call.arguments[3..7].iter().zip(point) {
                    memory.put::<i32>(offset, value);
                }
            }
            let (code, target) = event.map_or((0, 0), |event| {
                (
                    super::events::event_code(event.code),
                    state.event_handle_for(pointer, &event),
                )
            });
            memory.put::<u32>(b, code);
            memory.put_word(c, target);
        }
        Function::IndexedCanvasCreate => {
            state.ensure_capacity()?;
            let parent = state.object(a, None)?;
            let width = u16::try_from(b).map_err(|_| Error::InvalidValue)?;
            let height = u16::try_from(c).map_err(|_| Error::InvalidValue)?;
            state.validate_canvas_pixels(width as usize, height as usize)?;
            let canvas = super::canvas::IndexedCanvas::new(width as usize, height as usize)?;
            let object = unsafe { lvgl::lv_obj_create(parent) };
            let handle = state.insert(object, Kind::IndexedCanvas, Some(a))?;
            if let Err(error) = state.install_canvas(handle, canvas) {
                unsafe {
                    lvgl::lv_obj_delete(object);
                }
                return Err(error);
            }
            memory.put_word(call.result, handle);
        }
        Function::IndexedCanvasSetPixels => {
            let object = state.object(a, Some(Kind::IndexedCanvas))?;
            let colors = call.arguments[4];
            if colors == 0 || colors > 256 {
                return Err(Error::InvalidValue);
            }
            let pixels = memory.bytes(b, c)?;
            let palette = memory.values::<u32>(call.arguments[3], colors)?;
            state.canvas_mut(a).upload(pixels, palette)?;
            unsafe {
                lvgl::lv_obj_invalidate(object);
            }
        }
        Function::TextareaSetMaxLength => {
            let object = state.object(a, Some(Kind::Textarea))?;
            let length = u32::try_from(b).map_err(|_| Error::InvalidValue)?;
            if length > 4096 {
                return Err(Error::ResourceLimit);
            }
            unsafe { lvgl::lv_textarea_set_max_length(object, length) };
        }
        Function::TextareaGetText | Function::TextareaGetTextLength => {
            let object = state.object(a, Some(Kind::Textarea))?;
            let text = unsafe { core::ffi::CStr::from_ptr(lvgl::lv_textarea_get_text(object)) };
            let bytes = text.to_bytes_with_nul();
            let length = i32::try_from(bytes.len() - 1).map_err(|_| Error::ResourceLimit)?;
            if function == Function::TextareaGetText {
                if c < bytes.len() as Word {
                    return Err(Error::InvalidValue);
                }
                let output = memory.bytes_mut(b, c)?;
                output[..bytes.len()].copy_from_slice(bytes);
            }
            memory.put::<i32>(call.result, length);
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
            memory.put::<i32>(call.result, unsafe { lvgl::lv_pct(a as i32) });
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
        Function::ButtonmatrixSetMap => {
            state.object(a, Some(Kind::ButtonMatrix))?;
            let map = ButtonMap::decode(memory, b)?;
            state.set_map(a, map);
        }
        Function::ObjectSetFlexFlow => {
            let pointer = state.object(a, None)?;
            if !matches!(b, 0 | 1 | 4 | 5 | 8 | 9 | 12 | 13) {
                return Err(Error::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_flex_flow(pointer, b as _) };
        }
        _ => return Err(Error::Unsupported),
    }
    Ok(())
}
