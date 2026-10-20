use super::{
    Function, GraphicsState, ResultKind,
    error::{GraphicsError, Result},
    marshal::{ButtonMap, read_c_string, read_textarea_string},
    objects::Kind,
};
use crate::host::translation::{GuestMemory, GuestPointer, GuestSlice, WasmUsize};
use core::ptr::NonNull;
use xila::graphics::{Color, Window, lvgl};

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
        Function::WindowCreate
        | Function::ObjectCreate
        | Function::LabelCreate
        | Function::ButtonCreate
        | Function::CheckboxCreate
        | Function::TabviewCreate
        | Function::ButtonmatrixCreate
        | Function::TextareaCreate
        | Function::KeyboardCreate
        | Function::RollerCreate
        | Function::SwitchCreate
        | Function::ArcCreate
        | Function::ListCreate => {
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
                        Function::RollerCreate => (lvgl::lv_roller_create(parent), Kind::Roller),
                        Function::SwitchCreate => (lvgl::lv_switch_create(parent), Kind::Object),
                        Function::ArcCreate => (lvgl::lv_arc_create(parent), Kind::Arc),
                        Function::ListCreate => (lvgl::lv_list_create(parent), Kind::List),
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
        Function::RollerSetOptions => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let options = read_c_string(memory, b)?;
            if c > lvgl::lv_roller_mode_t_LV_ROLLER_MODE_INFINITE {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_roller_set_options(roller, options.as_ptr(), c as _) };
        }
        Function::RollerSetSelected | Function::RollerSetVisibleRowCount => {
            let roller = state.object(a, Some(Kind::Roller))?;
            if function == Function::RollerSetSelected {
                if c > 1 {
                    return Err(GraphicsError::InvalidValue);
                }
                let selected = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                unsafe { lvgl::lv_roller_set_selected(roller, selected, c != 0) };
            } else {
                let rows = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                if !(1..=9).contains(&rows) {
                    return Err(GraphicsError::InvalidValue);
                }
                unsafe { lvgl::lv_roller_set_visible_row_count(roller, rows) };
            }
        }
        Function::RollerGetSelected => {
            let roller = state.object(a, Some(Kind::Roller))?;
            let selected = unsafe { lvgl::lv_roller_get_selected(roller) };
            memory
                .write(GuestPointer::new(call.result), selected)
                .expect("validated result");
        }
        Function::ArcSetRange
        | Function::ArcSetValue
        | Function::ArcSetBgAngles
        | Function::ArcSetRotation
        | Function::ArcSetMode => {
            let arc = state.object(a, Some(Kind::Arc))?;
            unsafe {
                match function {
                    Function::ArcSetRange => {
                        let min = i32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        let max = i32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
                        if min >= max || min < 0 || max > 100_000 {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_arc_set_range(arc, min, max);
                    }
                    Function::ArcSetValue => {
                        let value = i32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        lvgl::lv_arc_set_value(arc, value);
                    }
                    Function::ArcSetBgAngles => {
                        let start = i32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        let end = i32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
                        if !(0..=360).contains(&start) || !(0..=360).contains(&end) {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_arc_set_bg_angles(arc, start, end);
                    }
                    Function::ArcSetRotation => {
                        let rotation = i32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        if !(0..=360).contains(&rotation) {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_arc_set_rotation(arc, rotation);
                    }
                    Function::ArcSetMode => {
                        if b > lvgl::lv_arc_mode_t_LV_ARC_MODE_REVERSE {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_arc_set_mode(arc, b as _);
                    }
                    _ => unreachable!(),
                }
            }
        }
        Function::ListAddText => {
            state.ensure_capacity()?;
            let list = state.object(a, Some(Kind::List))?;
            let text = read_c_string(memory, b)?;
            let label = unsafe { lvgl::lv_list_add_text(list, text.as_ptr()) };
            let handle = state.insert(label, Kind::Label, Some(a))?;
            memory
                .write(GuestPointer::new(call.result), handle)
                .expect("validated result");
        }
        Function::ObjectSetTextSize => {
            let object = state.object(a, None)?;
            let font = match b {
                10 => xila::graphics::fonts::get_font_small(),
                14 => xila::graphics::fonts::get_font_medium(),
                18 => xila::graphics::fonts::get_font_large(),
                28 => xila::graphics::fonts::get_font_extra_large(),
                _ => return Err(GraphicsError::InvalidValue),
            };
            unsafe { lvgl::lv_obj_set_style_text_font(object, font, 0) };
        }
        Function::ObjectSetStyleTextAlign => {
            let object = state.object(a, None)?;
            if b > lvgl::lv_text_align_t_LV_TEXT_ALIGN_RIGHT || c != 0 {
                return Err(GraphicsError::InvalidValue);
            }
            unsafe { lvgl::lv_obj_set_style_text_align(object, b as _, 0) };
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
        | Function::ObjectSetHeight
        | Function::ObjectSetWidth
        | Function::ObjectSetFlexAlign
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
                    Function::ObjectSetHeight => lvgl::lv_obj_set_height(pointer, b as i32),
                    Function::ObjectSetWidth => lvgl::lv_obj_set_width(pointer, b as i32),
                    Function::ObjectSetFlexAlign => {
                        let main_place =
                            u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        let cross_place =
                            u32::try_from(c).map_err(|_| GraphicsError::InvalidValue)?;
                        let track_place = u32::try_from(call.arguments[3])
                            .map_err(|_| GraphicsError::InvalidValue)?;
                        if main_place > lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN
                            || cross_place > lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN
                            || track_place > lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN
                        {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_obj_set_flex_align(
                            pointer,
                            main_place as _,
                            cross_place as _,
                            track_place as _,
                        );
                    }
                    Function::ObjectAddState => {
                        let state = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        if state & !lvgl::lv_state_t_LV_STATE_ANY != 0 {
                            return Err(GraphicsError::InvalidValue);
                        }
                        lvgl::lv_obj_add_state(pointer, state);
                    }
                    Function::ObjectRemoveState => {
                        let state = u32::try_from(b).map_err(|_| GraphicsError::InvalidValue)?;
                        if state & !lvgl::lv_state_t_LV_STATE_ANY != 0 {
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
    }
    Ok(())
}
