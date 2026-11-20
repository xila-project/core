/* Generated from the graphics ABI declaration. Do not edit. */
#include <stddef.h>
#include <stdint.h>

#include "stubs.h"
#include "xila/graphics.h"

/* A color or style value as one call word. */
static size_t xila_graphics_pack(const void *value, size_t size) {
    size_t word = 0;
    __builtin_memcpy(&word, value, size);
    return word;
}

int32_t xila_graphics_window_create(XilaGraphicsObject * *__result_pointer) {
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_WINDOW_CREATE, NULL, 0, __result_pointer);
}

int32_t xila_graphics_file_picker_create(XilaGraphicsObject * parent, uint32_t mode, char const * initial, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent, (size_t)mode, (size_t)initial};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_CREATE, arguments, 3, __result_pointer);
}

int32_t xila_graphics_file_picker_reset(XilaGraphicsObject * picker, uint32_t mode, char const * initial) {
    size_t arguments[] = {(size_t)picker, (size_t)mode, (size_t)initial};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_RESET, arguments, 3, NULL);
}

int32_t xila_graphics_file_picker_get_status(XilaGraphicsObject * picker, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)picker};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_GET_STATUS, arguments, 1, __result_pointer);
}

int32_t xila_graphics_file_picker_get_path(XilaGraphicsObject * picker, int8_t * buffer, size_t size, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)picker, (size_t)buffer, (size_t)size};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_GET_PATH, arguments, 3, __result_pointer);
}

int32_t xila_graphics_window_set_icon(XilaGraphicsObject * window, char const * icon_string, XilaGraphicsColor icon_color) {
    size_t arguments[] = {(size_t)window, (size_t)icon_string, xila_graphics_pack(&icon_color, sizeof(icon_color))};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_WINDOW_SET_ICON, arguments, 3, NULL);
}

int32_t xila_graphics_window_pop_event(XilaGraphicsObject * window, uint32_t * code, void * target) {
    size_t arguments[] = {(size_t)window, (size_t)code, (size_t)target};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_WINDOW_POP_EVENT, arguments, 3, NULL);
}

int32_t xila_graphics_percentage(int32_t value, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)value};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_PERCENTAGE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_delete(uint32_t object) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_DELETE, arguments, 1, NULL);
}

int32_t xila_graphics_object_set_flex_flow(XilaGraphicsObject * obj, XilaGraphicsFlexFlow flow) {
    size_t arguments[] = {(size_t)obj, (size_t)flow};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FLEX_FLOW, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_flex_grow(XilaGraphicsObject * obj, uint8_t grow) {
    size_t arguments[] = {(size_t)obj, (size_t)grow};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FLEX_GROW, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_height(XilaGraphicsObject * obj, int32_t h) {
    size_t arguments[] = {(size_t)obj, (size_t)h};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_HEIGHT, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_width(XilaGraphicsObject * obj, int32_t w) {
    size_t arguments[] = {(size_t)obj, (size_t)w};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_WIDTH, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_size(XilaGraphicsObject * obj, int32_t w, int32_t h) {
    size_t arguments[] = {(size_t)obj, (size_t)w, (size_t)h};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_SIZE, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_pos(XilaGraphicsObject * obj, int32_t x, int32_t y) {
    size_t arguments[] = {(size_t)obj, (size_t)x, (size_t)y};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_POS, arguments, 3, NULL);
}

int32_t xila_graphics_object_get_x(XilaGraphicsObject const * obj, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_X, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_get_y(XilaGraphicsObject const * obj, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_Y, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_get_width(XilaGraphicsObject const * obj, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_WIDTH, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_get_height(XilaGraphicsObject const * obj, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_HEIGHT, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_align(XilaGraphicsObject * obj, XilaGraphicsAlign align, int32_t x_ofs, int32_t y_ofs) {
    size_t arguments[] = {(size_t)obj, (size_t)align, (size_t)x_ofs, (size_t)y_ofs};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_ALIGN, arguments, 4, NULL);
}

int32_t xila_graphics_object_update_layout(XilaGraphicsObject const * obj) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_UPDATE_LAYOUT, arguments, 1, NULL);
}

int32_t xila_graphics_object_set_flex_align(XilaGraphicsObject * obj, XilaGraphicsFlexAlign main_place, XilaGraphicsFlexAlign cross_place, XilaGraphicsFlexAlign track_cross_place) {
    size_t arguments[] = {(size_t)obj, (size_t)main_place, (size_t)cross_place, (size_t)track_cross_place};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FLEX_ALIGN, arguments, 4, NULL);
}

int32_t xila_graphics_object_set_layout(XilaGraphicsObject * obj, uint32_t layout) {
    size_t arguments[] = {(size_t)obj, (size_t)layout};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_LAYOUT, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_grid_dsc_array(XilaGraphicsObject * obj, int32_t const * col_dsc, int32_t const * row_dsc) {
    size_t arguments[] = {(size_t)obj, (size_t)col_dsc, (size_t)row_dsc};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_GRID_DSC_ARRAY, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_grid_align(XilaGraphicsObject * obj, XilaGraphicsGridAlign column_align, XilaGraphicsGridAlign row_align) {
    size_t arguments[] = {(size_t)obj, (size_t)column_align, (size_t)row_align};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_GRID_ALIGN, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_grid_cell(XilaGraphicsObject * obj, XilaGraphicsGridAlign column_align, int32_t col_pos, int32_t col_span, XilaGraphicsGridAlign row_align, int32_t row_pos, int32_t row_span) {
    size_t arguments[] = {(size_t)obj, (size_t)column_align, (size_t)col_pos, (size_t)col_span, (size_t)row_align, (size_t)row_pos, (size_t)row_span};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_GRID_CELL, arguments, 7, NULL);
}

int32_t xila_graphics_grid_fr(uint8_t weight, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)weight};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_GRID_FR, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_add_state(XilaGraphicsObject * obj, XilaGraphicsState state) {
    size_t arguments[] = {(size_t)obj, (size_t)state};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_ADD_STATE, arguments, 2, NULL);
}

int32_t xila_graphics_object_get_state(XilaGraphicsObject const * obj, XilaGraphicsState *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_STATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_button_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_BUTTON_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_checkbox_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHECKBOX_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_checkbox_set_text(XilaGraphicsObject * obj, char const * txt) {
    size_t arguments[] = {(size_t)obj, (size_t)txt};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHECKBOX_SET_TEXT, arguments, 2, NULL);
}

int32_t xila_graphics_tabview_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TABVIEW_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_tabview_add_tab(XilaGraphicsObject * obj, char const * name, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)obj, (size_t)name};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TABVIEW_ADD_TAB, arguments, 2, __result_pointer);
}

int32_t xila_graphics_tabview_get_tab_active(XilaGraphicsObject * obj, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TABVIEW_GET_TAB_ACTIVE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_add_flag(XilaGraphicsObject * obj, XilaGraphicsObjectFlag f) {
    size_t arguments[] = {(size_t)obj, (size_t)f};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_ADD_FLAG, arguments, 2, NULL);
}

int32_t xila_graphics_label_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LABEL_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_label_set_text(XilaGraphicsObject * obj, char const * text) {
    size_t arguments[] = {(size_t)obj, (size_t)text};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LABEL_SET_TEXT, arguments, 2, NULL);
}

int32_t xila_graphics_buttonmatrix_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_BUTTONMATRIX_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_buttonmatrix_set_map(XilaGraphicsObject * obj, char const * const * map) {
    size_t arguments[] = {(size_t)obj, (size_t)map};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_BUTTONMATRIX_SET_MAP, arguments, 2, NULL);
}

int32_t xila_graphics_buttonmatrix_get_selected_button(XilaGraphicsObject const * obj, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_BUTTONMATRIX_GET_SELECTED_BUTTON, arguments, 1, __result_pointer);
}

int32_t xila_graphics_arc_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ARC_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_slider_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SLIDER_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_remove_state(XilaGraphicsObject * obj, XilaGraphicsState state) {
    size_t arguments[] = {(size_t)obj, (size_t)state};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_REMOVE_STATE, arguments, 2, NULL);
}

int32_t xila_graphics_dropdown_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_dropdown_set_options(XilaGraphicsObject * obj, char const * options) {
    size_t arguments[] = {(size_t)obj, (size_t)options};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_SET_OPTIONS, arguments, 2, NULL);
}

int32_t xila_graphics_dropdown_set_selected(XilaGraphicsObject * obj, uint32_t sel_opt) {
    size_t arguments[] = {(size_t)obj, (size_t)sel_opt};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_SET_SELECTED, arguments, 2, NULL);
}

int32_t xila_graphics_dropdown_get_selected(XilaGraphicsObject const * obj, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_SELECTED, arguments, 1, __result_pointer);
}

int32_t xila_graphics_dropdown_get_option_count(XilaGraphicsObject const * obj, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_OPTION_COUNT, arguments, 1, __result_pointer);
}

int32_t xila_graphics_dropdown_set_selected_highlight(XilaGraphicsObject * obj, bool en) {
    size_t arguments[] = {(size_t)obj, (size_t)en};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_SET_SELECTED_HIGHLIGHT, arguments, 2, NULL);
}

int32_t xila_graphics_dropdown_open(XilaGraphicsObject * dropdown_obj) {
    size_t arguments[] = {(size_t)dropdown_obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_OPEN, arguments, 1, NULL);
}

int32_t xila_graphics_dropdown_close(XilaGraphicsObject * obj) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_CLOSE, arguments, 1, NULL);
}

int32_t xila_graphics_roller_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_roller_set_options(XilaGraphicsObject * obj, char const * options, XilaGraphicsRollerMode mode) {
    size_t arguments[] = {(size_t)obj, (size_t)options, (size_t)mode};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_SET_OPTIONS, arguments, 3, NULL);
}

int32_t xila_graphics_roller_set_selected(XilaGraphicsObject * obj, uint32_t sel_opt, XilaGraphicsAnimEnable anim) {
    size_t arguments[] = {(size_t)obj, (size_t)sel_opt, (size_t)anim};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_SET_SELECTED, arguments, 3, NULL);
}

int32_t xila_graphics_roller_get_selected(XilaGraphicsObject const * obj, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_GET_SELECTED, arguments, 1, __result_pointer);
}

int32_t xila_graphics_roller_get_option_count(XilaGraphicsObject const * obj, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_GET_OPTION_COUNT, arguments, 1, __result_pointer);
}

int32_t xila_graphics_roller_set_visible_row_count(XilaGraphicsObject * obj, uint32_t row_cnt) {
    size_t arguments[] = {(size_t)obj, (size_t)row_cnt};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_SET_VISIBLE_ROW_COUNT, arguments, 2, NULL);
}

int32_t xila_graphics_calendar_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_calendar_set_month_shown(XilaGraphicsObject * obj, uint32_t year, uint32_t month) {
    size_t arguments[] = {(size_t)obj, (size_t)year, (size_t)month};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_SET_MONTH_SHOWN, arguments, 3, NULL);
}

int32_t xila_graphics_spinbox_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_spinbox_set_range(XilaGraphicsObject * obj, int32_t min_value, int32_t max_value) {
    size_t arguments[] = {(size_t)obj, (size_t)min_value, (size_t)max_value};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_RANGE, arguments, 3, NULL);
}

int32_t xila_graphics_spinbox_set_value(XilaGraphicsObject * obj, int32_t v) {
    size_t arguments[] = {(size_t)obj, (size_t)v};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_VALUE, arguments, 2, NULL);
}

int32_t xila_graphics_spinbox_get_value(XilaGraphicsObject * obj, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_GET_VALUE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_spinbox_set_step(XilaGraphicsObject * obj, uint32_t step) {
    size_t arguments[] = {(size_t)obj, (size_t)step};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_STEP, arguments, 2, NULL);
}

int32_t xila_graphics_spinbox_set_digit_format(XilaGraphicsObject * obj, uint32_t digit_count, uint32_t sep_pos) {
    size_t arguments[] = {(size_t)obj, (size_t)digit_count, (size_t)sep_pos};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_DIGIT_FORMAT, arguments, 3, NULL);
}

int32_t xila_graphics_spinbox_set_rollover(XilaGraphicsObject * obj, bool rollover) {
    size_t arguments[] = {(size_t)obj, (size_t)rollover};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_ROLLOVER, arguments, 2, NULL);
}

int32_t xila_graphics_spinbox_increment(XilaGraphicsObject * obj) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_INCREMENT, arguments, 1, NULL);
}

int32_t xila_graphics_spinbox_decrement(XilaGraphicsObject * obj) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_DECREMENT, arguments, 1, NULL);
}

int32_t xila_graphics_chart_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_chart_set_type(XilaGraphicsObject * obj, XilaGraphicsChartType type_) {
    size_t arguments[] = {(size_t)obj, (size_t)type_};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_TYPE, arguments, 2, NULL);
}

int32_t xila_graphics_chart_set_point_count(XilaGraphicsObject * obj, uint32_t cnt) {
    size_t arguments[] = {(size_t)obj, (size_t)cnt};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_POINT_COUNT, arguments, 2, NULL);
}

int32_t xila_graphics_chart_set_axis_range(XilaGraphicsObject * obj, XilaGraphicsChartAxis axis, int32_t min, int32_t max) {
    size_t arguments[] = {(size_t)obj, (size_t)axis, (size_t)min, (size_t)max};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_AXIS_RANGE, arguments, 4, NULL);
}

int32_t xila_graphics_line_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LINE_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_line_set_points(XilaGraphicsObject * obj, XilaGraphicsPointPrecise const * points, uint32_t point_num) {
    size_t arguments[] = {(size_t)obj, (size_t)points, (size_t)point_num};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LINE_SET_POINTS, arguments, 3, NULL);
}

int32_t xila_graphics_line_set_y_invert(XilaGraphicsObject * obj, bool en) {
    size_t arguments[] = {(size_t)obj, (size_t)en};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LINE_SET_Y_INVERT, arguments, 2, NULL);
}

int32_t xila_graphics_tileview_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_tileview_add_tile(XilaGraphicsObject * tv, uint8_t col_id, uint8_t row_id, XilaGraphicsDir dir, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)tv, (size_t)col_id, (size_t)row_id, (size_t)dir};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_ADD_TILE, arguments, 4, __result_pointer);
}

int32_t xila_graphics_tileview_set_tile(XilaGraphicsObject * obj, XilaGraphicsObject * tile_obj, XilaGraphicsAnimEnable anim_en) {
    size_t arguments[] = {(size_t)obj, (size_t)tile_obj, (size_t)anim_en};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_SET_TILE, arguments, 3, NULL);
}

int32_t xila_graphics_tileview_get_tile_active(XilaGraphicsObject * obj, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_GET_TILE_ACTIVE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_object_set_padding(XilaGraphicsObject * object, int32_t padding) {
    size_t arguments[] = {(size_t)object, (size_t)padding};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_PADDING, arguments, 2, NULL);
}

int32_t xila_graphics_indexed_canvas_create(XilaGraphicsObject * parent, uint16_t width, uint16_t height, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent, (size_t)width, (size_t)height};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_INDEXED_CANVAS_CREATE, arguments, 3, __result_pointer);
}

int32_t xila_graphics_indexed_canvas_set_pixels(XilaGraphicsObject * canvas, uint8_t const * pixels, size_t length, uint32_t const * palette, size_t colors) {
    size_t arguments[] = {(size_t)canvas, (size_t)pixels, (size_t)length, (size_t)palette, (size_t)colors};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_INDEXED_CANVAS_SET_PIXELS, arguments, 5, NULL);
}

int32_t xila_graphics_window_pop_pointer_event(XilaGraphicsObject * window, uint32_t * code, void * target, int32_t * x, int32_t * y, int32_t * width, int32_t * height) {
    size_t arguments[] = {(size_t)window, (size_t)code, (size_t)target, (size_t)x, (size_t)y, (size_t)width, (size_t)height};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_WINDOW_POP_POINTER_EVENT, arguments, 7, NULL);
}

int32_t xila_graphics_textarea_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_textarea_set_one_line(XilaGraphicsObject * obj, bool en) {
    size_t arguments[] = {(size_t)obj, (size_t)en};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_SET_ONE_LINE, arguments, 2, NULL);
}

int32_t xila_graphics_textarea_set_text(XilaGraphicsObject * obj, char const * txt) {
    size_t arguments[] = {(size_t)obj, (size_t)txt};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_SET_TEXT, arguments, 2, NULL);
}

int32_t xila_graphics_textarea_set_max_length(XilaGraphicsObject * obj, uint32_t num) {
    size_t arguments[] = {(size_t)obj, (size_t)num};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_SET_MAX_LENGTH, arguments, 2, NULL);
}

int32_t xila_graphics_textarea_get_text(XilaGraphicsObject * textarea, int8_t * buffer, size_t buffer_size, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)textarea, (size_t)buffer, (size_t)buffer_size};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_GET_TEXT, arguments, 3, __result_pointer);
}

int32_t xila_graphics_textarea_get_text_length(XilaGraphicsObject * textarea, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)textarea};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_GET_TEXT_LENGTH, arguments, 1, __result_pointer);
}

int32_t xila_graphics_keyboard_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_KEYBOARD_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_keyboard_set_textarea(XilaGraphicsObject * keyboard, XilaGraphicsObject * textarea) {
    size_t arguments[] = {(size_t)keyboard, (size_t)textarea};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_KEYBOARD_SET_TEXTAREA, arguments, 2, NULL);
}

int32_t xila_graphics_object_remove_flag(XilaGraphicsObject * obj, XilaGraphicsObjectFlag f) {
    size_t arguments[] = {(size_t)obj, (size_t)f};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_REMOVE_FLAG, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_background(XilaGraphicsObject * object, uint32_t rgb, uint8_t opacity, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)rgb, (size_t)opacity, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_BACKGROUND, arguments, 4, NULL);
}

int32_t xila_graphics_object_set_text_color(XilaGraphicsObject * object, uint32_t rgb, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)rgb, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_TEXT_COLOR, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_border(XilaGraphicsObject * object, uint32_t rgb, int32_t width, uint32_t side, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)rgb, (size_t)width, (size_t)side, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_BORDER, arguments, 5, NULL);
}

int32_t xila_graphics_object_set_radius(XilaGraphicsObject * object, int32_t radius, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)radius, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_RADIUS, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_text_align(XilaGraphicsObject * object, uint32_t align, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)align, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_TEXT_ALIGN, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_font(XilaGraphicsObject * object, uint32_t font, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)font, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FONT, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_surface(XilaGraphicsObject * object, uint32_t surface) {
    size_t arguments[] = {(size_t)object, (size_t)surface};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_SURFACE, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_min_max_size(XilaGraphicsObject * object, int32_t min_width, int32_t max_width, int32_t min_height, int32_t max_height, uint32_t selector) {
    size_t arguments[] = {(size_t)object, (size_t)min_width, (size_t)max_width, (size_t)min_height, (size_t)max_height, (size_t)selector};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_MIN_MAX_SIZE, arguments, 6, NULL);
}

int32_t xila_graphics_bar_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_BAR_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_switch_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SWITCH_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_spinner_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINNER_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_led_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LED_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_led_set_brightness(XilaGraphicsObject * obj, uint8_t bright) {
    size_t arguments[] = {(size_t)obj, (size_t)bright};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LED_SET_BRIGHTNESS, arguments, 2, NULL);
}

int32_t xila_graphics_led_on(XilaGraphicsObject * led) {
    size_t arguments[] = {(size_t)led};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LED_ON, arguments, 1, NULL);
}

int32_t xila_graphics_led_off(XilaGraphicsObject * led) {
    size_t arguments[] = {(size_t)led};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LED_OFF, arguments, 1, NULL);
}

int32_t xila_graphics_led_toggle(XilaGraphicsObject * obj) {
    size_t arguments[] = {(size_t)obj};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LED_TOGGLE, arguments, 1, NULL);
}

int32_t xila_graphics_range_set_range(XilaGraphicsObject * object, int32_t min, int32_t max) {
    size_t arguments[] = {(size_t)object, (size_t)min, (size_t)max};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_RANGE, arguments, 3, NULL);
}

int32_t xila_graphics_range_set_value(XilaGraphicsObject * object, int32_t value, bool animated) {
    size_t arguments[] = {(size_t)object, (size_t)value, (size_t)animated};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_VALUE, arguments, 3, NULL);
}

int32_t xila_graphics_range_get_value(XilaGraphicsObject const * object, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_RANGE_GET_VALUE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_range_get_start_value(XilaGraphicsObject const * object, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_RANGE_GET_START_VALUE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_range_set_mode(XilaGraphicsObject * object, uint32_t mode) {
    size_t arguments[] = {(size_t)object, (size_t)mode};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_MODE, arguments, 2, NULL);
}

int32_t xila_graphics_range_set_orientation(XilaGraphicsObject * object, uint32_t orientation) {
    size_t arguments[] = {(size_t)object, (size_t)orientation};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_ORIENTATION, arguments, 2, NULL);
}

int32_t xila_graphics_spinner_set_animation(XilaGraphicsObject * object, uint32_t duration_ms, uint32_t sweep_degrees) {
    size_t arguments[] = {(size_t)object, (size_t)duration_ms, (size_t)sweep_degrees};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINNER_SET_ANIMATION, arguments, 3, NULL);
}

int32_t xila_graphics_spinner_get_animation_duration(XilaGraphicsObject const * object, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINNER_GET_ANIMATION_DURATION, arguments, 1, __result_pointer);
}

int32_t xila_graphics_spinner_get_arc_sweep(XilaGraphicsObject const * object, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_SPINNER_GET_ARC_SWEEP, arguments, 1, __result_pointer);
}

int32_t xila_graphics_dropdown_get_selected_string(XilaGraphicsObject * object, int8_t * output, uint32_t capacity, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object, (size_t)output, (size_t)capacity};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_SELECTED_STRING, arguments, 3, __result_pointer);
}

int32_t xila_graphics_roller_get_selected_string(XilaGraphicsObject * object, int8_t * output, uint32_t capacity, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object, (size_t)output, (size_t)capacity};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_ROLLER_GET_SELECTED_STRING, arguments, 3, __result_pointer);
}

int32_t xila_graphics_dropdown_get_highlight(XilaGraphicsObject * object, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_HIGHLIGHT, arguments, 1, __result_pointer);
}

int32_t xila_graphics_dropdown_is_open_state(XilaGraphicsObject * object, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)object};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_IS_OPEN_STATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_calendar_set_today(XilaGraphicsObject * calendar, uint32_t year, uint32_t month, uint32_t day) {
    size_t arguments[] = {(size_t)calendar, (size_t)year, (size_t)month, (size_t)day};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_SET_TODAY, arguments, 4, NULL);
}

int32_t xila_graphics_calendar_set_highlights(XilaGraphicsObject * calendar, uint8_t const * dates, size_t count) {
    size_t arguments[] = {(size_t)calendar, (size_t)dates, (size_t)count};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_SET_HIGHLIGHTS, arguments, 3, NULL);
}

int32_t xila_graphics_calendar_get_shown_date(XilaGraphicsObject * calendar, uint8_t * output) {
    size_t arguments[] = {(size_t)calendar, (size_t)output};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_GET_SHOWN_DATE, arguments, 2, NULL);
}

int32_t xila_graphics_chart_add_series_slot(XilaGraphicsObject * chart, uint32_t rgb, uint32_t *__result_pointer) {
    size_t arguments[] = {(size_t)chart, (size_t)rgb};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_ADD_SERIES_SLOT, arguments, 2, __result_pointer);
}

int32_t xila_graphics_chart_set_values(XilaGraphicsObject * chart, uint32_t series_slot, int32_t const * values, size_t count) {
    size_t arguments[] = {(size_t)chart, (size_t)series_slot, (size_t)values, (size_t)count};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_VALUES, arguments, 4, NULL);
}

int32_t xila_graphics_chart_get_value(XilaGraphicsObject * chart, uint32_t series_slot, uint32_t index, int32_t *__result_pointer) {
    size_t arguments[] = {(size_t)chart, (size_t)series_slot, (size_t)index};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CHART_GET_VALUE, arguments, 3, __result_pointer);
}

int32_t xila_graphics_list_add_text_button(XilaGraphicsObject * list, char const * text, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)list, (size_t)text};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LIST_ADD_TEXT_BUTTON, arguments, 2, __result_pointer);
}

int32_t xila_graphics_list_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LIST_CREATE, arguments, 1, __result_pointer);
}

int32_t xila_graphics_list_add_text(XilaGraphicsObject * list, char const * text, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)list, (size_t)text};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LIST_ADD_TEXT, arguments, 2, __result_pointer);
}

int32_t xila_graphics_list_set_button_text(XilaGraphicsObject * list, XilaGraphicsObject * button, char const * text) {
    size_t arguments[] = {(size_t)list, (size_t)button, (size_t)text};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_LIST_SET_BUTTON_TEXT, arguments, 3, NULL);
}

int32_t xila_graphics_object_set_theme_variant(XilaGraphicsObject * object, uint32_t variant) {
    size_t arguments[] = {(size_t)object, (size_t)variant};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_THEME_VARIANT, arguments, 2, NULL);
}

int32_t xila_graphics_object_set_theme_error(XilaGraphicsObject * object, bool error) {
    size_t arguments[] = {(size_t)object, (size_t)error};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_THEME_ERROR, arguments, 2, NULL);
}

int32_t xila_graphics_badge_create(XilaGraphicsObject * parent, char const * text, uint32_t variant, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent, (size_t)text, (size_t)variant};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_BADGE_CREATE, arguments, 3, __result_pointer);
}

int32_t xila_graphics_card_create(XilaGraphicsObject * parent, XilaGraphicsObject * *__result_pointer) {
    size_t arguments[] = {(size_t)parent};
    return (int32_t)xila_graphics_call(xila_sdk_process, XILA_GRAPHICS_FUNCTION_CALL_CARD_CREATE, arguments, 1, __result_pointer);
}
