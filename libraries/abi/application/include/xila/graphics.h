#ifndef XILA_GRAPHICS_H
#define XILA_GRAPHICS_H

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>

#define XilaGraphicsFLEX_COLUMN (1 << 0)

#define XilaGraphicsFLEX_WRAP (1 << 2)

#define XilaGraphicsFLEX_REVERSE (1 << 3)

#define XilaGraphicsPART_MAIN 0

#define XilaGraphicsPART_SCROLLBAR 65536

#define XilaGraphicsPART_INDICATOR 131072

#define XilaGraphicsPART_KNOB 196608

#define XilaGraphicsPART_SELECTED 262144

#define XilaGraphicsPART_ITEMS 327680

#define XilaGraphicsPART_CURSOR 393216

#define XilaGraphicsPART_CUSTOM_FIRST 524288

#define XilaGraphicsPART_ANY 983040

#define XilaGraphicsCOORD_TYPE_SHIFT 29

#define XilaGraphicsCOORD_TYPE_MASK (3 << XilaGraphicsCOORD_TYPE_SHIFT)

#define XilaGraphicsCOORD_TYPE_PX (0 << XilaGraphicsCOORD_TYPE_SHIFT)

#define XilaGraphicsCOORD_TYPE_SPEC (1 << XilaGraphicsCOORD_TYPE_SHIFT)

#define XilaGraphicsCOORD_TYPE_PX_NEG (3 << XilaGraphicsCOORD_TYPE_SHIFT)

#define XilaGraphicsCOORD_MAX ((1 << XilaGraphicsCOORD_TYPE_SHIFT) - 1)

#define XilaGraphicsCOORD_MIN -XilaGraphicsCOORD_MAX

/**
 * Xila window close request, separate from LVGL's standard events.
 */
#define XilaGraphicsEVENT_CLOSE_REQUESTED ((uint32_t)XilaGraphicsEventCode_Last + 3)

typedef enum XilaGraphicsFlexFlow {
  XILA_GRAPHICS_FLEX_FLOW_ROW = 0,
  XILA_GRAPHICS_FLEX_FLOW_COLUMN = (ptrdiff_t)XilaGraphicsFLEX_COLUMN,
  XILA_GRAPHICS_FLEX_FLOW_ROW_WRAP = (ptrdiff_t)XilaGraphicsFLEX_WRAP,
  XILA_GRAPHICS_FLEX_FLOW_ROW_REVERSE = (ptrdiff_t)XilaGraphicsFLEX_REVERSE,
  XILA_GRAPHICS_FLEX_FLOW_ROW_WRAP_REVERSE = ((ptrdiff_t)XilaGraphicsFLEX_WRAP | (ptrdiff_t)XilaGraphicsFLEX_REVERSE),
  XILA_GRAPHICS_FLEX_FLOW_COLUMN_WRAP = ((ptrdiff_t)XilaGraphicsFLEX_COLUMN | (ptrdiff_t)XilaGraphicsFLEX_WRAP),
  XILA_GRAPHICS_FLEX_FLOW_COLUMN_REVERSE = ((ptrdiff_t)XilaGraphicsFLEX_COLUMN | (ptrdiff_t)XilaGraphicsFLEX_REVERSE),
  XILA_GRAPHICS_FLEX_FLOW_COLUMN_WRAP_REVERSE = (((ptrdiff_t)XilaGraphicsFLEX_COLUMN | (ptrdiff_t)XilaGraphicsFLEX_WRAP) | (ptrdiff_t)XilaGraphicsFLEX_REVERSE),
} XilaGraphicsFlexFlow;

typedef enum XilaGraphicsAlign {
  XILA_GRAPHICS_ALIGN_DEFAULT = 0,
  XILA_GRAPHICS_ALIGN_TOP_LEFT,
  XILA_GRAPHICS_ALIGN_TOP_MID,
  XILA_GRAPHICS_ALIGN_TOP_RIGHT,
  XILA_GRAPHICS_ALIGN_BOTTOM_LEFT,
  XILA_GRAPHICS_ALIGN_BOTTOM_MID,
  XILA_GRAPHICS_ALIGN_BOTTOM_RIGHT,
  XILA_GRAPHICS_ALIGN_LEFT_MID,
  XILA_GRAPHICS_ALIGN_RIGHT_MID,
  XILA_GRAPHICS_ALIGN_CENTER,
  XILA_GRAPHICS_ALIGN_OUT_TOP_LEFT,
  XILA_GRAPHICS_ALIGN_OUT_TOP_MID,
  XILA_GRAPHICS_ALIGN_OUT_TOP_RIGHT,
  XILA_GRAPHICS_ALIGN_OUT_BOTTOM_LEFT,
  XILA_GRAPHICS_ALIGN_OUT_BOTTOM_MID,
  XILA_GRAPHICS_ALIGN_OUT_BOTTOM_RIGHT,
  XILA_GRAPHICS_ALIGN_OUT_LEFT_TOP,
  XILA_GRAPHICS_ALIGN_OUT_LEFT_MID,
  XILA_GRAPHICS_ALIGN_OUT_LEFT_BOTTOM,
  XILA_GRAPHICS_ALIGN_OUT_RIGHT_TOP,
  XILA_GRAPHICS_ALIGN_OUT_RIGHT_MID,
  XILA_GRAPHICS_ALIGN_OUT_RIGHT_BOTTOM,
} XilaGraphicsAlign;

typedef enum XilaGraphicsFlexAlign {
  XILA_GRAPHICS_FLEX_ALIGN_START,
  XILA_GRAPHICS_FLEX_ALIGN_END,
  XILA_GRAPHICS_FLEX_ALIGN_CENTER,
  XILA_GRAPHICS_FLEX_ALIGN_SPACE_EVENLY,
  XILA_GRAPHICS_FLEX_ALIGN_SPACE_AROUND,
  XILA_GRAPHICS_FLEX_ALIGN_SPACE_BETWEEN,
} XilaGraphicsFlexAlign;

typedef enum XilaGraphicsGridAlign {
  XILA_GRAPHICS_GRID_ALIGN_START,
  XILA_GRAPHICS_GRID_ALIGN_CENTER,
  XILA_GRAPHICS_GRID_ALIGN_END,
  XILA_GRAPHICS_GRID_ALIGN_STRETCH,
  XILA_GRAPHICS_GRID_ALIGN_SPACE_EVENLY,
  XILA_GRAPHICS_GRID_ALIGN_SPACE_AROUND,
  XILA_GRAPHICS_GRID_ALIGN_SPACE_BETWEEN,
} XilaGraphicsGridAlign;

typedef enum XilaGraphicsObjectFlag {
  XILA_GRAPHICS_OBJECT_FLAG_HIDDEN = (1 << 0),
  XILA_GRAPHICS_OBJECT_FLAG_CLICKABLE = (1 << 1),
  XILA_GRAPHICS_OBJECT_FLAG_CLICK_FOCUSABLE = (1 << 2),
  XILA_GRAPHICS_OBJECT_FLAG_CHECKABLE = (1 << 3),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLLABLE = (1 << 4),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_ELASTIC = (1 << 5),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_MOMENTUM = (1 << 6),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_ONE = (1 << 7),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_CHAIN_HOR = (1 << 8),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_CHAIN_VER = (1 << 9),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_CHAIN = ((1 << 8) | (1 << 9)),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_ON_FOCUS = (1 << 10),
  XILA_GRAPHICS_OBJECT_FLAG_SCROLL_WITH_ARROW = (1 << 11),
  XILA_GRAPHICS_OBJECT_FLAG_SNAPPABLE = (1 << 12),
  XILA_GRAPHICS_OBJECT_FLAG_PRESS_LOCK = (1 << 13),
  XILA_GRAPHICS_OBJECT_FLAG_EVENT_BUBBLE = (1 << 14),
  XILA_GRAPHICS_OBJECT_FLAG_GESTURE_BUBBLE = (1 << 15),
  XILA_GRAPHICS_OBJECT_FLAG_ADV_HITTEST = (1 << 16),
  XILA_GRAPHICS_OBJECT_FLAG_IGNORE_LAYOUT = (1 << 17),
  XILA_GRAPHICS_OBJECT_FLAG_FLOATING = (1 << 18),
  XILA_GRAPHICS_OBJECT_FLAG_SEND_DRAW_TASK_EVENTS = (1 << 19),
  XILA_GRAPHICS_OBJECT_FLAG_OVERFLOW_VISIBLE = (1 << 20),
  XILA_GRAPHICS_OBJECT_FLAG_FLEX_IN_NEW_TRACK = (1 << 21),
  XILA_GRAPHICS_OBJECT_FLAG_LAYOUT1 = (1 << 23),
  XILA_GRAPHICS_OBJECT_FLAG_LAYOUT2 = (1 << 24),
  XILA_GRAPHICS_OBJECT_FLAG_WIDGET1 = (1 << 25),
  XILA_GRAPHICS_OBJECT_FLAG_WIDGET2 = (1 << 26),
  XILA_GRAPHICS_OBJECT_FLAG_USER1 = (1 << 27),
  XILA_GRAPHICS_OBJECT_FLAG_USER2 = (1 << 28),
  XILA_GRAPHICS_OBJECT_FLAG_USER3 = (1 << 29),
  XILA_GRAPHICS_OBJECT_FLAG_USER4 = (1 << 30),
} XilaGraphicsObjectFlag;

typedef enum XilaGraphicsRollerMode {
  XILA_GRAPHICS_ROLLER_MODE_NORMAL,
  XILA_GRAPHICS_ROLLER_MODE_INFINITE,
} XilaGraphicsRollerMode;

typedef enum XilaGraphicsAnimEnable {
  XILA_GRAPHICS_ANIM_ENABLE_OFF,
  XILA_GRAPHICS_ANIM_ENABLE_ON,
} XilaGraphicsAnimEnable;

/**
 * Mirrors LVGL's `lv_chart_type_t`; the host forwards the value unchanged.
 */
typedef enum XilaGraphicsChartType {
  XILA_GRAPHICS_CHART_TYPE_NONE,
  XILA_GRAPHICS_CHART_TYPE_LINE,
  XILA_GRAPHICS_CHART_TYPE_CURVE,
  XILA_GRAPHICS_CHART_TYPE_BAR,
  XILA_GRAPHICS_CHART_TYPE_STACKED,
  XILA_GRAPHICS_CHART_TYPE_SCATTER,
} XilaGraphicsChartType;

typedef enum XilaGraphicsChartAxis {
  XILA_GRAPHICS_CHART_AXIS_PRIMARY_Y = 0,
  XILA_GRAPHICS_CHART_AXIS_SECONDARY_Y = 1,
  XILA_GRAPHICS_CHART_AXIS_PRIMARY_X = 2,
  XILA_GRAPHICS_CHART_AXIS_SECONDARY_X = 4,
  XILA_GRAPHICS_CHART_AXIS_LAST,
} XilaGraphicsChartAxis;

typedef enum XilaGraphicsDir {
  XILA_GRAPHICS_DIR_NONE = 0,
  XILA_GRAPHICS_DIR_LEFT = (1 << 0),
  XILA_GRAPHICS_DIR_RIGHT = (1 << 1),
  XILA_GRAPHICS_DIR_TOP = (1 << 2),
  XILA_GRAPHICS_DIR_BOTTOM = (1 << 3),
  XILA_GRAPHICS_DIR_HOR = ((1 << 0) | (1 << 1)),
  XILA_GRAPHICS_DIR_VER = ((1 << 2) | (1 << 3)),
  XILA_GRAPHICS_DIR_ALL = ((((1 << 0) | (1 << 1)) | (1 << 2)) | (1 << 3)),
} XilaGraphicsDir;

typedef enum XilaGraphicsEventCode {
  XILA_GRAPHICS_EVENT_CODE_ALL = 0,
  XILA_GRAPHICS_EVENT_CODE_PRESSED,
  XILA_GRAPHICS_EVENT_CODE_PRESSING,
  XILA_GRAPHICS_EVENT_CODE_PRESS_LOST,
  XILA_GRAPHICS_EVENT_CODE_SHORT_CLICKED,
  XILA_GRAPHICS_EVENT_CODE_SINGLE_CLICKED,
  XILA_GRAPHICS_EVENT_CODE_DOUBLE_CLICKED,
  XILA_GRAPHICS_EVENT_CODE_TRIPLE_CLICKED,
  XILA_GRAPHICS_EVENT_CODE_LONG_PRESSED,
  XILA_GRAPHICS_EVENT_CODE_LONG_PRESSED_REPEAT,
  XILA_GRAPHICS_EVENT_CODE_CLICKED,
  XILA_GRAPHICS_EVENT_CODE_RELEASED,
  XILA_GRAPHICS_EVENT_CODE_SCROLL_BEGIN,
  XILA_GRAPHICS_EVENT_CODE_SCROLL_THROW_BEGIN,
  XILA_GRAPHICS_EVENT_CODE_SCROLL_END,
  XILA_GRAPHICS_EVENT_CODE_SCROLL,
  XILA_GRAPHICS_EVENT_CODE_GESTURE,
  XILA_GRAPHICS_EVENT_CODE_KEY,
  XILA_GRAPHICS_EVENT_CODE_ROTARY,
  XILA_GRAPHICS_EVENT_CODE_FOCUSED,
  XILA_GRAPHICS_EVENT_CODE_DEFOCUSED,
  XILA_GRAPHICS_EVENT_CODE_LEAVE,
  XILA_GRAPHICS_EVENT_CODE_HIT_TEST,
  XILA_GRAPHICS_EVENT_CODE_INDEV_RESET,
  XILA_GRAPHICS_EVENT_CODE_HOVER_OVER,
  XILA_GRAPHICS_EVENT_CODE_HOVER_LEAVE,
  XILA_GRAPHICS_EVENT_CODE_COVER_CHECK,
  XILA_GRAPHICS_EVENT_CODE_REFRESH_EXT_DRAW_SIZE,
  XILA_GRAPHICS_EVENT_CODE_DRAW_MAIN_BEGIN,
  XILA_GRAPHICS_EVENT_CODE_DRAW_MAIN,
  XILA_GRAPHICS_EVENT_CODE_DRAW_MAIN_END,
  XILA_GRAPHICS_EVENT_CODE_DRAW_POST_BEGIN,
  XILA_GRAPHICS_EVENT_CODE_DRAW_POST,
  XILA_GRAPHICS_EVENT_CODE_DRAW_POST_END,
  XILA_GRAPHICS_EVENT_CODE_DRAW_TASK_ADDED,
  XILA_GRAPHICS_EVENT_CODE_VALUE_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_INSERT,
  XILA_GRAPHICS_EVENT_CODE_REFRESH,
  XILA_GRAPHICS_EVENT_CODE_READY,
  XILA_GRAPHICS_EVENT_CODE_CANCEL,
  XILA_GRAPHICS_EVENT_CODE_CREATE,
  XILA_GRAPHICS_EVENT_CODE_DELETE,
  XILA_GRAPHICS_EVENT_CODE_CHILD_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_CHILD_CREATED,
  XILA_GRAPHICS_EVENT_CODE_CHILD_DELETED,
  XILA_GRAPHICS_EVENT_CODE_SCREEN_UNLOAD_START,
  XILA_GRAPHICS_EVENT_CODE_SCREEN_LOAD_START,
  XILA_GRAPHICS_EVENT_CODE_SCREEN_LOADED,
  XILA_GRAPHICS_EVENT_CODE_SCREEN_UNLOADED,
  XILA_GRAPHICS_EVENT_CODE_SIZE_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_STYLE_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_LAYOUT_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_GET_SELF_SIZE,
  XILA_GRAPHICS_EVENT_CODE_INVALIDATE_AREA,
  XILA_GRAPHICS_EVENT_CODE_RESOLUTION_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_COLOR_FORMAT_CHANGED,
  XILA_GRAPHICS_EVENT_CODE_REFRESH_REQUEST,
  XILA_GRAPHICS_EVENT_CODE_REFRESH_START,
  XILA_GRAPHICS_EVENT_CODE_REFRESH_READY,
  XILA_GRAPHICS_EVENT_CODE_RENDER_START,
  XILA_GRAPHICS_EVENT_CODE_RENDER_READY,
  XILA_GRAPHICS_EVENT_CODE_FLUSH_START,
  XILA_GRAPHICS_EVENT_CODE_FLUSH_FINISH,
  XILA_GRAPHICS_EVENT_CODE_FLUSH_WAIT_START,
  XILA_GRAPHICS_EVENT_CODE_FLUSH_WAIT_FINISH,
  XILA_GRAPHICS_EVENT_CODE_VSYNC,
  XILA_GRAPHICS_EVENT_CODE_VSYNC_REQUEST,
  XILA_GRAPHICS_EVENT_CODE_LAST,
  XILA_GRAPHICS_EVENT_CODE_PREPROCESS = 32768,
  XILA_GRAPHICS_EVENT_CODE_MARKED_DELETING = 65536,
} XilaGraphicsEventCode;

enum XilaGraphicsFunctionCall
#if defined(__cplusplus) || __STDC_VERSION__ >= 202311L
  : uint16_t
#endif // defined(__cplusplus) || __STDC_VERSION__ >= 202311L
 {
  XILA_GRAPHICS_FUNCTION_CALL_WINDOW_CREATE = 1037,
  XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_CREATE = 61485,
  XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_RESET = 61488,
  XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_GET_STATUS = 61486,
  XILA_GRAPHICS_FUNCTION_CALL_FILE_PICKER_GET_PATH = 61487,
  XILA_GRAPHICS_FUNCTION_CALL_WINDOW_SET_ICON = 1042,
  XILA_GRAPHICS_FUNCTION_CALL_WINDOW_POP_EVENT = 1041,
  XILA_GRAPHICS_FUNCTION_CALL_PERCENTAGE = 1034,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_CREATE = 248,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_DELETE = 1033,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FLEX_FLOW = 554,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FLEX_GROW = 555,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_HEIGHT = 564,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_WIDTH = 736,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_SIZE = 591,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_POS = 572,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_X = 443,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_Y = 446,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_WIDTH = 442,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_HEIGHT = 266,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_ALIGN = 236,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_UPDATE_LAYOUT = 753,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FLEX_ALIGN = 553,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_LAYOUT = 568,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_GRID_DSC_ARRAY = 563,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_GRID_ALIGN = 561,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_GRID_CELL = 562,
  XILA_GRAPHICS_FUNCTION_CALL_GRID_FR = 61446,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_ADD_STATE = 234,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_GET_STATE = 288,
  XILA_GRAPHICS_FUNCTION_CALL_BUTTON_CREATE = 28,
  XILA_GRAPHICS_FUNCTION_CALL_CHECKBOX_CREATE = 100,
  XILA_GRAPHICS_FUNCTION_CALL_CHECKBOX_SET_TEXT = 102,
  XILA_GRAPHICS_FUNCTION_CALL_TABVIEW_CREATE = 980,
  XILA_GRAPHICS_FUNCTION_CALL_TABVIEW_ADD_TAB = 979,
  XILA_GRAPHICS_FUNCTION_CALL_TABVIEW_GET_TAB_ACTIVE = 982,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_ADD_FLAG = 232,
  XILA_GRAPHICS_FUNCTION_CALL_LABEL_CREATE = 160,
  XILA_GRAPHICS_FUNCTION_CALL_LABEL_SET_TEXT = 175,
  XILA_GRAPHICS_FUNCTION_CALL_BUTTONMATRIX_CREATE = 31,
  XILA_GRAPHICS_FUNCTION_CALL_BUTTONMATRIX_SET_MAP = 40,
  XILA_GRAPHICS_FUNCTION_CALL_BUTTONMATRIX_GET_SELECTED_BUTTON = 34,
  XILA_GRAPHICS_FUNCTION_CALL_ARC_CREATE = 1,
  XILA_GRAPHICS_FUNCTION_CALL_SLIDER_CREATE = 776,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_REMOVE_STATE = 520,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_CREATE = 139,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_SET_OPTIONS = 153,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_SET_SELECTED = 155,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_SELECTED = 145,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_OPTION_COUNT = 142,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_SET_SELECTED_HIGHLIGHT = 156,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_OPEN = 151,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_CLOSE = 138,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_CREATE = 763,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_SET_OPTIONS = 769,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_SET_SELECTED = 770,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_GET_SELECTED = 767,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_GET_OPTION_COUNT = 764,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_SET_VISIBLE_ROW_COUNT = 772,
  XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_CREATE = 45,
  XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_SET_MONTH_SHOWN = 55,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_CREATE = 792,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_RANGE = 810,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_VALUE = 813,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_GET_VALUE = 801,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_STEP = 812,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_DIGIT_FORMAT = 806,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_SET_ROLLOVER = 811,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_INCREMENT = 802,
  XILA_GRAPHICS_FUNCTION_CALL_SPINBOX_DECREMENT = 793,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_CREATE = 63,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_TYPE = 96,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_POINT_COUNT = 88,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_AXIS_RANGE = 83,
  XILA_GRAPHICS_FUNCTION_CALL_LINE_CREATE = 188,
  XILA_GRAPHICS_FUNCTION_CALL_LINE_SET_POINTS = 194,
  XILA_GRAPHICS_FUNCTION_CALL_LINE_SET_Y_INVERT = 196,
  XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_CREATE = 1029,
  XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_ADD_TILE = 1028,
  XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_SET_TILE = 1031,
  XILA_GRAPHICS_FUNCTION_CALL_TILEVIEW_GET_TILE_ACTIVE = 1030,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_PADDING = 61445,
  XILA_GRAPHICS_FUNCTION_CALL_INDEXED_CANVAS_CREATE = 61440,
  XILA_GRAPHICS_FUNCTION_CALL_INDEXED_CANVAS_SET_PIXELS = 61441,
  XILA_GRAPHICS_FUNCTION_CALL_WINDOW_POP_POINTER_EVENT = 61442,
  XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_CREATE = 994,
  XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_SET_ONE_LINE = 1020,
  XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_SET_TEXT = 1025,
  XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_SET_MAX_LENGTH = 1019,
  XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_GET_TEXT = 1035,
  XILA_GRAPHICS_FUNCTION_CALL_TEXTAREA_GET_TEXT_LENGTH = 1036,
  XILA_GRAPHICS_FUNCTION_CALL_KEYBOARD_CREATE = 61443,
  XILA_GRAPHICS_FUNCTION_CALL_KEYBOARD_SET_TEXTAREA = 61444,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_REMOVE_FLAG = 518,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_BACKGROUND = 61447,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_TEXT_COLOR = 61448,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_BORDER = 61449,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_RADIUS = 61450,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_TEXT_ALIGN = 61451,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_FONT = 61452,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_SURFACE = 61453,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_MIN_MAX_SIZE = 61454,
  XILA_GRAPHICS_FUNCTION_CALL_BAR_CREATE = 61455,
  XILA_GRAPHICS_FUNCTION_CALL_SWITCH_CREATE = 61456,
  XILA_GRAPHICS_FUNCTION_CALL_SPINNER_CREATE = 61457,
  XILA_GRAPHICS_FUNCTION_CALL_LED_CREATE = 180,
  XILA_GRAPHICS_FUNCTION_CALL_LED_SET_BRIGHTNESS = 185,
  XILA_GRAPHICS_FUNCTION_CALL_LED_ON = 184,
  XILA_GRAPHICS_FUNCTION_CALL_LED_OFF = 183,
  XILA_GRAPHICS_FUNCTION_CALL_LED_TOGGLE = 187,
  XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_RANGE = 61458,
  XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_VALUE = 61459,
  XILA_GRAPHICS_FUNCTION_CALL_RANGE_GET_VALUE = 61460,
  XILA_GRAPHICS_FUNCTION_CALL_RANGE_GET_START_VALUE = 61461,
  XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_MODE = 61462,
  XILA_GRAPHICS_FUNCTION_CALL_RANGE_SET_ORIENTATION = 61463,
  XILA_GRAPHICS_FUNCTION_CALL_SPINNER_SET_ANIMATION = 61464,
  XILA_GRAPHICS_FUNCTION_CALL_SPINNER_GET_ANIMATION_DURATION = 61465,
  XILA_GRAPHICS_FUNCTION_CALL_SPINNER_GET_ARC_SWEEP = 61466,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_SELECTED_STRING = 61467,
  XILA_GRAPHICS_FUNCTION_CALL_ROLLER_GET_SELECTED_STRING = 61468,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_GET_HIGHLIGHT = 61469,
  XILA_GRAPHICS_FUNCTION_CALL_DROPDOWN_IS_OPEN_STATE = 61470,
  XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_SET_TODAY = 61471,
  XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_SET_HIGHLIGHTS = 61472,
  XILA_GRAPHICS_FUNCTION_CALL_CALENDAR_GET_SHOWN_DATE = 61473,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_ADD_SERIES_SLOT = 61474,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_SET_VALUES = 61475,
  XILA_GRAPHICS_FUNCTION_CALL_CHART_GET_VALUE = 61476,
  XILA_GRAPHICS_FUNCTION_CALL_LIST_ADD_TEXT_BUTTON = 61479,
  XILA_GRAPHICS_FUNCTION_CALL_LIST_CREATE = 61477,
  XILA_GRAPHICS_FUNCTION_CALL_LIST_ADD_TEXT = 61478,
  XILA_GRAPHICS_FUNCTION_CALL_LIST_SET_BUTTON_TEXT = 61480,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_THEME_VARIANT = 61481,
  XILA_GRAPHICS_FUNCTION_CALL_OBJECT_SET_THEME_ERROR = 61482,
  XILA_GRAPHICS_FUNCTION_CALL_BADGE_CREATE = 61483,
  XILA_GRAPHICS_FUNCTION_CALL_CARD_CREATE = 61484,
};
#ifndef __cplusplus
#if __STDC_VERSION__ >= 202311L
typedef enum XilaGraphicsFunctionCall XilaGraphicsFunctionCall;
#else
typedef uint16_t XilaGraphicsFunctionCall;
#endif // __STDC_VERSION__ >= 202311L
#endif // __cplusplus

typedef struct XilaGraphicsObject {
  uint8_t _private[0];
} XilaGraphicsObject;

typedef struct XilaGraphicsColor {
  uint8_t blue;
  uint8_t green;
  uint8_t red;
} XilaGraphicsColor;

typedef unsigned int XilaGraphicsState;

typedef int32_t XilaGraphicsValuePrecise;

typedef struct XilaGraphicsPointPrecise {
  XilaGraphicsValuePrecise x;
  XilaGraphicsValuePrecise y;
} XilaGraphicsPointPrecise;

#define XilaGraphicsSTATE_DEFAULT 0

#define XilaGraphicsSTATE_ALT 1

#define XilaGraphicsSTATE_CHECKED 4

#define XilaGraphicsSTATE_FOCUSED 8

#define XilaGraphicsSTATE_FOCUS_KEY 16

#define XilaGraphicsSTATE_EDITED 32

#define XilaGraphicsSTATE_HOVERED 64

#define XilaGraphicsSTATE_PRESSED 128

#define XilaGraphicsSTATE_SCROLLED 256

#define XilaGraphicsSTATE_DISABLED 512

#define XilaGraphicsSTATE_USER_1 4096

#define XilaGraphicsSTATE_USER_2 8192

#define XilaGraphicsSTATE_USER_3 16384

#define XilaGraphicsSTATE_USER_4 32768

#define XilaGraphicsSTATE_ANY 65535

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

int32_t xila_graphics_window_create(struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_file_picker_create(struct XilaGraphicsObject *parent,
                                         uint32_t mode,
                                         const char *initial,
                                         struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_file_picker_reset(struct XilaGraphicsObject *picker,
                                        uint32_t mode,
                                        const char *initial);

int32_t xila_graphics_file_picker_get_status(struct XilaGraphicsObject *picker,
                                             uint32_t *__result_pointer);

int32_t xila_graphics_file_picker_get_path(struct XilaGraphicsObject *picker,
                                           int8_t *buffer,
                                           size_t size,
                                           int32_t *__result_pointer);

int32_t xila_graphics_window_set_icon(struct XilaGraphicsObject *window,
                                      const char *icon_string,
                                      struct XilaGraphicsColor icon_color);

int32_t xila_graphics_window_pop_event(struct XilaGraphicsObject *window,
                                       uint32_t *code,
                                       void *target);

int32_t xila_graphics_percentage(int32_t value, int32_t *__result_pointer);

int32_t xila_graphics_object_create(struct XilaGraphicsObject *parent,
                                    struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_object_delete(uint32_t object);

int32_t xila_graphics_object_set_flex_flow(struct XilaGraphicsObject *obj,
                                           enum XilaGraphicsFlexFlow flow);

int32_t xila_graphics_object_set_flex_grow(struct XilaGraphicsObject *obj, uint8_t grow);

int32_t xila_graphics_object_set_height(struct XilaGraphicsObject *obj, int32_t h);

int32_t xila_graphics_object_set_width(struct XilaGraphicsObject *obj, int32_t w);

int32_t xila_graphics_object_set_size(struct XilaGraphicsObject *obj, int32_t w, int32_t h);

int32_t xila_graphics_object_set_pos(struct XilaGraphicsObject *obj, int32_t x, int32_t y);

int32_t xila_graphics_object_get_x(const struct XilaGraphicsObject *obj, int32_t *__result_pointer);

int32_t xila_graphics_object_get_y(const struct XilaGraphicsObject *obj, int32_t *__result_pointer);

int32_t xila_graphics_object_get_width(const struct XilaGraphicsObject *obj,
                                       int32_t *__result_pointer);

int32_t xila_graphics_object_get_height(const struct XilaGraphicsObject *obj,
                                        int32_t *__result_pointer);

int32_t xila_graphics_object_align(struct XilaGraphicsObject *obj,
                                   enum XilaGraphicsAlign align,
                                   int32_t x_ofs,
                                   int32_t y_ofs);

int32_t xila_graphics_object_update_layout(const struct XilaGraphicsObject *obj);

int32_t xila_graphics_object_set_flex_align(struct XilaGraphicsObject *obj,
                                            enum XilaGraphicsFlexAlign main_place,
                                            enum XilaGraphicsFlexAlign cross_place,
                                            enum XilaGraphicsFlexAlign track_cross_place);

int32_t xila_graphics_object_set_layout(struct XilaGraphicsObject *obj, uint32_t layout);

int32_t xila_graphics_object_set_grid_dsc_array(struct XilaGraphicsObject *obj,
                                                const int32_t *col_dsc,
                                                const int32_t *row_dsc);

int32_t xila_graphics_object_set_grid_align(struct XilaGraphicsObject *obj,
                                            enum XilaGraphicsGridAlign column_align,
                                            enum XilaGraphicsGridAlign row_align);

int32_t xila_graphics_object_set_grid_cell(struct XilaGraphicsObject *obj,
                                           enum XilaGraphicsGridAlign column_align,
                                           int32_t col_pos,
                                           int32_t col_span,
                                           enum XilaGraphicsGridAlign row_align,
                                           int32_t row_pos,
                                           int32_t row_span);

int32_t xila_graphics_grid_fr(uint8_t weight, int32_t *__result_pointer);

int32_t xila_graphics_object_add_state(struct XilaGraphicsObject *obj, XilaGraphicsState state);

int32_t xila_graphics_object_get_state(const struct XilaGraphicsObject *obj,
                                       XilaGraphicsState *__result_pointer);

int32_t xila_graphics_button_create(struct XilaGraphicsObject *parent,
                                    struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_checkbox_create(struct XilaGraphicsObject *parent,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_checkbox_set_text(struct XilaGraphicsObject *obj, const char *txt);

int32_t xila_graphics_tabview_create(struct XilaGraphicsObject *parent,
                                     struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_tabview_add_tab(struct XilaGraphicsObject *obj,
                                      const char *name,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_tabview_get_tab_active(struct XilaGraphicsObject *obj,
                                             uint32_t *__result_pointer);

int32_t xila_graphics_object_add_flag(struct XilaGraphicsObject *obj,
                                      enum XilaGraphicsObjectFlag f);

int32_t xila_graphics_label_create(struct XilaGraphicsObject *parent,
                                   struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_label_set_text(struct XilaGraphicsObject *obj, const char *text);

int32_t xila_graphics_buttonmatrix_create(struct XilaGraphicsObject *parent,
                                          struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_buttonmatrix_set_map(struct XilaGraphicsObject *obj, const char *const *map);

int32_t xila_graphics_buttonmatrix_get_selected_button(const struct XilaGraphicsObject *obj,
                                                       uint32_t *__result_pointer);

int32_t xila_graphics_arc_create(struct XilaGraphicsObject *parent,
                                 struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_slider_create(struct XilaGraphicsObject *parent,
                                    struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_object_remove_state(struct XilaGraphicsObject *obj, XilaGraphicsState state);

int32_t xila_graphics_dropdown_create(struct XilaGraphicsObject *parent,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_dropdown_set_options(struct XilaGraphicsObject *obj, const char *options);

int32_t xila_graphics_dropdown_set_selected(struct XilaGraphicsObject *obj, uint32_t sel_opt);

int32_t xila_graphics_dropdown_get_selected(const struct XilaGraphicsObject *obj,
                                            uint32_t *__result_pointer);

int32_t xila_graphics_dropdown_get_option_count(const struct XilaGraphicsObject *obj,
                                                uint32_t *__result_pointer);

int32_t xila_graphics_dropdown_set_selected_highlight(struct XilaGraphicsObject *obj, bool en);

int32_t xila_graphics_dropdown_open(struct XilaGraphicsObject *dropdown_obj);

int32_t xila_graphics_dropdown_close(struct XilaGraphicsObject *obj);

int32_t xila_graphics_roller_create(struct XilaGraphicsObject *parent,
                                    struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_roller_set_options(struct XilaGraphicsObject *obj,
                                         const char *options,
                                         enum XilaGraphicsRollerMode mode);

int32_t xila_graphics_roller_set_selected(struct XilaGraphicsObject *obj,
                                          uint32_t sel_opt,
                                          enum XilaGraphicsAnimEnable anim);

int32_t xila_graphics_roller_get_selected(const struct XilaGraphicsObject *obj,
                                          uint32_t *__result_pointer);

int32_t xila_graphics_roller_get_option_count(const struct XilaGraphicsObject *obj,
                                              uint32_t *__result_pointer);

int32_t xila_graphics_roller_set_visible_row_count(struct XilaGraphicsObject *obj,
                                                   uint32_t row_cnt);

int32_t xila_graphics_calendar_create(struct XilaGraphicsObject *parent,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_calendar_set_month_shown(struct XilaGraphicsObject *obj,
                                               uint32_t year,
                                               uint32_t month);

int32_t xila_graphics_spinbox_create(struct XilaGraphicsObject *parent,
                                     struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_spinbox_set_range(struct XilaGraphicsObject *obj,
                                        int32_t min_value,
                                        int32_t max_value);

int32_t xila_graphics_spinbox_set_value(struct XilaGraphicsObject *obj, int32_t v);

int32_t xila_graphics_spinbox_get_value(struct XilaGraphicsObject *obj, int32_t *__result_pointer);

int32_t xila_graphics_spinbox_set_step(struct XilaGraphicsObject *obj, uint32_t step);

int32_t xila_graphics_spinbox_set_digit_format(struct XilaGraphicsObject *obj,
                                               uint32_t digit_count,
                                               uint32_t sep_pos);

int32_t xila_graphics_spinbox_set_rollover(struct XilaGraphicsObject *obj, bool rollover);

int32_t xila_graphics_spinbox_increment(struct XilaGraphicsObject *obj);

int32_t xila_graphics_spinbox_decrement(struct XilaGraphicsObject *obj);

int32_t xila_graphics_chart_create(struct XilaGraphicsObject *parent,
                                   struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_chart_set_type(struct XilaGraphicsObject *obj,
                                     enum XilaGraphicsChartType type_);

int32_t xila_graphics_chart_set_point_count(struct XilaGraphicsObject *obj, uint32_t cnt);

int32_t xila_graphics_chart_set_axis_range(struct XilaGraphicsObject *obj,
                                           enum XilaGraphicsChartAxis axis,
                                           int32_t min,
                                           int32_t max);

int32_t xila_graphics_line_create(struct XilaGraphicsObject *parent,
                                  struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_line_set_points(struct XilaGraphicsObject *obj,
                                      const struct XilaGraphicsPointPrecise *points,
                                      uint32_t point_num);

int32_t xila_graphics_line_set_y_invert(struct XilaGraphicsObject *obj, bool en);

int32_t xila_graphics_tileview_create(struct XilaGraphicsObject *parent,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_tileview_add_tile(struct XilaGraphicsObject *tv,
                                        uint8_t col_id,
                                        uint8_t row_id,
                                        enum XilaGraphicsDir dir,
                                        struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_tileview_set_tile(struct XilaGraphicsObject *obj,
                                        struct XilaGraphicsObject *tile_obj,
                                        enum XilaGraphicsAnimEnable anim_en);

int32_t xila_graphics_tileview_get_tile_active(struct XilaGraphicsObject *obj,
                                               struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_object_set_padding(struct XilaGraphicsObject *object, int32_t padding);

int32_t xila_graphics_indexed_canvas_create(struct XilaGraphicsObject *parent,
                                            uint16_t width,
                                            uint16_t height,
                                            struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_indexed_canvas_set_pixels(struct XilaGraphicsObject *canvas,
                                                const uint8_t *pixels,
                                                size_t length,
                                                const uint32_t *palette,
                                                size_t colors);

int32_t xila_graphics_window_pop_pointer_event(struct XilaGraphicsObject *window,
                                               uint32_t *code,
                                               void *target,
                                               int32_t *x,
                                               int32_t *y,
                                               int32_t *width,
                                               int32_t *height);

int32_t xila_graphics_textarea_create(struct XilaGraphicsObject *parent,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_textarea_set_one_line(struct XilaGraphicsObject *obj, bool en);

int32_t xila_graphics_textarea_set_text(struct XilaGraphicsObject *obj, const char *txt);

int32_t xila_graphics_textarea_set_max_length(struct XilaGraphicsObject *obj, uint32_t num);

int32_t xila_graphics_textarea_get_text(struct XilaGraphicsObject *textarea,
                                        int8_t *buffer,
                                        size_t buffer_size,
                                        int32_t *__result_pointer);

int32_t xila_graphics_textarea_get_text_length(struct XilaGraphicsObject *textarea,
                                               int32_t *__result_pointer);

int32_t xila_graphics_keyboard_create(struct XilaGraphicsObject *parent,
                                      struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_keyboard_set_textarea(struct XilaGraphicsObject *keyboard,
                                            struct XilaGraphicsObject *textarea);

int32_t xila_graphics_object_remove_flag(struct XilaGraphicsObject *obj,
                                         enum XilaGraphicsObjectFlag f);

int32_t xila_graphics_object_set_background(struct XilaGraphicsObject *object,
                                            uint32_t rgb,
                                            uint8_t opacity,
                                            uint32_t selector);

int32_t xila_graphics_object_set_text_color(struct XilaGraphicsObject *object,
                                            uint32_t rgb,
                                            uint32_t selector);

int32_t xila_graphics_object_set_border(struct XilaGraphicsObject *object,
                                        uint32_t rgb,
                                        int32_t width,
                                        uint32_t side,
                                        uint32_t selector);

int32_t xila_graphics_object_set_radius(struct XilaGraphicsObject *object,
                                        int32_t radius,
                                        uint32_t selector);

int32_t xila_graphics_object_set_text_align(struct XilaGraphicsObject *object,
                                            uint32_t align,
                                            uint32_t selector);

int32_t xila_graphics_object_set_font(struct XilaGraphicsObject *object,
                                      uint32_t font,
                                      uint32_t selector);

int32_t xila_graphics_object_set_surface(struct XilaGraphicsObject *object, uint32_t surface);

int32_t xila_graphics_object_set_min_max_size(struct XilaGraphicsObject *object,
                                              int32_t min_width,
                                              int32_t max_width,
                                              int32_t min_height,
                                              int32_t max_height,
                                              uint32_t selector);

int32_t xila_graphics_bar_create(struct XilaGraphicsObject *parent,
                                 struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_switch_create(struct XilaGraphicsObject *parent,
                                    struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_spinner_create(struct XilaGraphicsObject *parent,
                                     struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_led_create(struct XilaGraphicsObject *parent,
                                 struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_led_set_brightness(struct XilaGraphicsObject *obj, uint8_t bright);

int32_t xila_graphics_led_on(struct XilaGraphicsObject *led);

int32_t xila_graphics_led_off(struct XilaGraphicsObject *led);

int32_t xila_graphics_led_toggle(struct XilaGraphicsObject *obj);

int32_t xila_graphics_range_set_range(struct XilaGraphicsObject *object, int32_t min, int32_t max);

int32_t xila_graphics_range_set_value(struct XilaGraphicsObject *object,
                                      int32_t value,
                                      bool animated);

int32_t xila_graphics_range_get_value(const struct XilaGraphicsObject *object,
                                      int32_t *__result_pointer);

int32_t xila_graphics_range_get_start_value(const struct XilaGraphicsObject *object,
                                            int32_t *__result_pointer);

int32_t xila_graphics_range_set_mode(struct XilaGraphicsObject *object, uint32_t mode);

int32_t xila_graphics_range_set_orientation(struct XilaGraphicsObject *object,
                                            uint32_t orientation);

int32_t xila_graphics_spinner_set_animation(struct XilaGraphicsObject *object,
                                            uint32_t duration_ms,
                                            uint32_t sweep_degrees);

int32_t xila_graphics_spinner_get_animation_duration(const struct XilaGraphicsObject *object,
                                                     uint32_t *__result_pointer);

int32_t xila_graphics_spinner_get_arc_sweep(const struct XilaGraphicsObject *object,
                                            uint32_t *__result_pointer);

int32_t xila_graphics_dropdown_get_selected_string(struct XilaGraphicsObject *object,
                                                   int8_t *output,
                                                   uint32_t capacity,
                                                   int32_t *__result_pointer);

int32_t xila_graphics_roller_get_selected_string(struct XilaGraphicsObject *object,
                                                 int8_t *output,
                                                 uint32_t capacity,
                                                 int32_t *__result_pointer);

int32_t xila_graphics_dropdown_get_highlight(struct XilaGraphicsObject *object,
                                             uint32_t *__result_pointer);

int32_t xila_graphics_dropdown_is_open_state(struct XilaGraphicsObject *object,
                                             uint32_t *__result_pointer);

int32_t xila_graphics_calendar_set_today(struct XilaGraphicsObject *calendar,
                                         uint32_t year,
                                         uint32_t month,
                                         uint32_t day);

int32_t xila_graphics_calendar_set_highlights(struct XilaGraphicsObject *calendar,
                                              const uint8_t *dates,
                                              size_t count);

int32_t xila_graphics_calendar_get_shown_date(struct XilaGraphicsObject *calendar, uint8_t *output);

int32_t xila_graphics_chart_add_series_slot(struct XilaGraphicsObject *chart,
                                            uint32_t rgb,
                                            uint32_t *__result_pointer);

int32_t xila_graphics_chart_set_values(struct XilaGraphicsObject *chart,
                                       uint32_t series_slot,
                                       const int32_t *values,
                                       size_t count);

int32_t xila_graphics_chart_get_value(struct XilaGraphicsObject *chart,
                                      uint32_t series_slot,
                                      uint32_t index,
                                      int32_t *__result_pointer);

int32_t xila_graphics_list_add_text_button(struct XilaGraphicsObject *list,
                                           const char *text,
                                           struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_list_create(struct XilaGraphicsObject *parent,
                                  struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_list_add_text(struct XilaGraphicsObject *list,
                                    const char *text,
                                    struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_list_set_button_text(struct XilaGraphicsObject *list,
                                           struct XilaGraphicsObject *button,
                                           const char *text);

int32_t xila_graphics_object_set_theme_variant(struct XilaGraphicsObject *object, uint32_t variant);

int32_t xila_graphics_object_set_theme_error(struct XilaGraphicsObject *object, bool error);

int32_t xila_graphics_badge_create(struct XilaGraphicsObject *parent,
                                   const char *text,
                                   uint32_t variant,
                                   struct XilaGraphicsObject **__result_pointer);

int32_t xila_graphics_card_create(struct XilaGraphicsObject *parent,
                                  struct XilaGraphicsObject **__result_pointer);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* XILA_GRAPHICS_H */
