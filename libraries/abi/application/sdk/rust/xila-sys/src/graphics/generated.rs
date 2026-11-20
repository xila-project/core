// Generated from the graphics ABI declaration. Do not edit.

use super::call;
use core::ffi::c_void;
/// System file picker selection policy. Selection never creates or truncates a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FilePickerMode {
    OpenFile = 0,
    SaveFile = 1,
    Directory = 2,
}
/// A result remains available until the caller deletes the picker object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FilePickerStatus {
    Pending = 0,
    Selected = 1,
    Cancelled = 2,
}
pub type Error = i32;
pub type Result<T> = core::result::Result<T, Error>;
#[repr(C)]
pub struct Object {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Color {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Color16 {
    _bitfield: u16,
}
impl Color16 {
    pub fn new(red: u8, green: u8, blue: u8) -> Self {
        let red = (red as u16 & 0x1F) << 11;
        let green = (green as u16 & 0x3F) << 5;
        let blue = blue as u16 & 0x1F;
        Self {
            _bitfield: red | green | blue,
        }
    }
    pub fn red(&self) -> u8 {
        ((self._bitfield >> 11) & 0x1F) as u8
    }
    pub fn green(&self) -> u8 {
        ((self._bitfield >> 5) & 0x3F) as u8
    }
    pub fn blue(&self) -> u8 {
        (self._bitfield & 0x1F) as u8
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Color32 {
    pub blue: u8,
    pub green: u8,
    pub red: u8,
    pub alpha: u8,
}
#[repr(C)]
pub struct ColorHsv {
    pub h: u16,
    pub s: u8,
    pub v: u8,
}
#[repr(C)]
pub struct Color16a {
    pub lumi: u8,
    pub alpha: u8,
}
#[repr(C)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
pub struct Area {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}
pub type ValuePrecise = i32;
#[repr(C)]
pub struct PointPrecise {
    pub x: ValuePrecise,
    pub y: ValuePrecise,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorFormat {
    Unknown = 0,
    Raw = 1,
    RawAlpha = 2,
    L8 = 6,
    I1 = 7,
    I2 = 8,
    I4 = 9,
    I8 = 10,
    A8 = 14,
    Rgb565 = 18,
    Argb8565 = 19,
    Rgb565a8 = 20,
    Al88 = 21,
    Rgb565Swapped = 27,
    Rgb888 = 15,
    Argb8888 = 16,
    Xrgb8888 = 17,
    Argb8888Premultiplied = 26,
    A1 = 11,
    A2 = 12,
    A4 = 13,
    Argb1555 = 22,
    Argb4444 = 23,
    Argb2222 = 24,
    I420 = 32,
    I422 = 33,
    I444 = 34,
    I400 = 35,
    Nv21 = 36,
    Nv12 = 37,
    Yuy2 = 38,
    Uyvy = 39,
    NemaTsc4 = 48,
    NemaTsc6 = 49,
    NemaTsc6a = 50,
    NemaTsc6ap = 51,
    NemaTsc12 = 52,
    NemaTsc12a = 53,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexAlign {
    Start,
    End,
    Center,
    SpaceEvenly,
    SpaceAround,
    SpaceBetween,
}
pub const FLEX_COLUMN: u8 = 1 << 0;
pub const FLEX_WRAP: u8 = 1 << 2;
pub const FLEX_REVERSE: u8 = 1 << 3;
pub type StyleProp = u8;
pub type Opa = u8;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexFlow {
    Row = 0x00,
    Column = FLEX_COLUMN as isize,
    RowWrap = FLEX_WRAP as isize,
    RowReverse = FLEX_REVERSE as isize,
    RowWrapReverse = FLEX_WRAP as isize | FLEX_REVERSE as isize,
    ColumnWrap = FLEX_COLUMN as isize | FLEX_WRAP as isize,
    ColumnReverse = FLEX_COLUMN as isize | FLEX_REVERSE as isize,
    ColumnWrapReverse = FLEX_COLUMN as isize | FLEX_WRAP as isize
        | FLEX_REVERSE as isize,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradDir {
    None,
    Ver,
    Hor,
    Linear,
    Radial,
    Conical,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Additive,
    Subtractive,
    Multiply,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDecor {
    None = 0x00,
    Underline = 0x01,
    Strikethrough = 0x02,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderSide {
    None = 0x00,
    Bottom = 0x01,
    Top = 0x02,
    Left = 0x04,
    Right = 0x08,
    Full = 0x0F,
    Internal = 0x10,
}
pub type Anim = u8;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleStateCmp {
    Same,
    DiffRedraw,
    DiffDrawPad,
    DiffLayout,
}
pub type StyleSelector = u32;
pub type ObjectHandle = u32;
pub type LvObject = u32;
pub type Part = u32;
#[repr(C)]
pub struct ChartSeries {
    pub x_points: *mut i32,
    pub y_points: *mut i32,
    pub color: Color,
    pub start_point: u32,
    _bitfield: u32,
}
impl ChartSeries {
    pub fn hidden(&self) -> bool {
        (self._bitfield & 0x01) != 0
    }
    pub fn set_hidden(&mut self, value: bool) {
        if value {
            self._bitfield |= 0x01;
        } else {
            self._bitfield &= !0x01;
        }
    }
    pub fn x_ext_buf_assigned(&self) -> bool {
        (self._bitfield & 0x02) != 0
    }
    pub fn y_ext_buf_assigned(&self) -> bool {
        (self._bitfield & 0x04) != 0
    }
    pub fn x_axis_sec(&self) -> bool {
        (self._bitfield & 0x08) != 0
    }
    pub fn y_axis_sec(&self) -> bool {
        (self._bitfield & 0x10) != 0
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Mirrors LVGL's `lv_chart_type_t`; the host forwards the value unchanged.
pub enum ChartType {
    None,
    Line,
    Curve,
    Bar,
    Stacked,
    Scatter,
}
pub type Style = *mut core::ffi::c_void;
#[repr(C)]
pub struct Font {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    None = 0x00,
    Left = 1 << 0,
    Right = 1 << 1,
    Top = 1 << 2,
    Bottom = 1 << 3,
    Hor = (1 << 0) | (1 << 1),
    Ver = (1 << 2) | (1 << 3),
    All = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3),
}
#[repr(C)]
pub struct CalendarDate {
    pub year: u16,
    pub month: i8,
    pub day: i8,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Default = 0,
    TopLeft,
    TopMid,
    TopRight,
    BottomLeft,
    BottomMid,
    BottomRight,
    LeftMid,
    RightMid,
    Center,
    OutTopLeft,
    OutTopMid,
    OutTopRight,
    OutBottomLeft,
    OutBottomMid,
    OutBottomRight,
    OutLeftTop,
    OutLeftMid,
    OutLeftBottom,
    OutRightTop,
    OutRightMid,
    OutRightBottom,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAlign {
    Start,
    Center,
    End,
    Stretch,
    SpaceEvenly,
    SpaceAround,
    SpaceBetween,
}
pub type ObjectClass = u16;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonmatrixCtrl {
    Hidden = 0x0010,
    NoRepeat = 0x0020,
    Disabled = 0x0040,
    Checkable = 0x0080,
    Checked = 0x0100,
    ClickTrig = 0x0200,
    Popover = 0x0400,
    Reserved1 = 0x0800,
    Reserved2 = 0x1000,
    Reserved3 = 0x2000,
    Custom1 = 0x4000,
    Custom2 = 0x8000,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArcMode {
    Normal,
    Symmetrical,
    Reverse,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuModeHeader {
    TopFixed,
    TopUnfixed,
    BottomFixed,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuModeRootBackButton {
    Disabled,
    Enabled,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollerMode {
    Normal,
    Infinite,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableCellCtrl {
    MergeRight = 1 << 0,
    TextCrop = 1 << 1,
    Custom1 = 1 << 4,
    Custom2 = 1 << 5,
    Custom3 = 1 << 6,
    Custom4 = 1 << 7,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFlag {
    Hidden = 1 << 0,
    Clickable = 1 << 1,
    ClickFocusable = 1 << 2,
    Checkable = 1 << 3,
    Scrollable = 1 << 4,
    ScrollElastic = 1 << 5,
    ScrollMomentum = 1 << 6,
    ScrollOne = 1 << 7,
    ScrollChainHor = 1 << 8,
    ScrollChainVer = 1 << 9,
    ScrollChain = (1 << 8) | (1 << 9),
    ScrollOnFocus = 1 << 10,
    ScrollWithArrow = 1 << 11,
    Snappable = 1 << 12,
    PressLock = 1 << 13,
    EventBubble = 1 << 14,
    GestureBubble = 1 << 15,
    AdvHittest = 1 << 16,
    IgnoreLayout = 1 << 17,
    Floating = 1 << 18,
    SendDrawTaskEvents = 1 << 19,
    OverflowVisible = 1 << 20,
    FlexInNewTrack = 1 << 21,
    Layout1 = 1 << 23,
    Layout2 = 1 << 24,
    Widget1 = 1 << 25,
    Widget2 = 1 << 26,
    User1 = 1 << 27,
    User2 = 1 << 28,
    User3 = 1 << 29,
    User4 = 1 << 30,
}
pub const PART_MAIN: u32 = 0x000000;
pub const PART_SCROLLBAR: u32 = 0x010000;
pub const PART_INDICATOR: u32 = 0x020000;
pub const PART_KNOB: u32 = 0x030000;
pub const PART_SELECTED: u32 = 0x040000;
pub const PART_ITEMS: u32 = 0x050000;
pub const PART_CURSOR: u32 = 0x060000;
pub const PART_CUSTOM_FIRST: u32 = 0x080000;
pub const PART_ANY: u32 = 0x0F0000;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFlag {
    None = 0x00,
    Expand = 0x01,
    Fit = 0x02,
    BreakAll = 0x04,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Auto,
    Left,
    Center,
    Right,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectPointTransformFlag {
    None = 0x00,
    Recursive = 0x01,
    Inverse = 0x02,
    InverseRecursive = 0x03,
}
#[repr(C)]
pub struct Group {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimEnable {
    Off,
    On,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseDir {
    Ltr = 0x00,
    Rtl = 0x01,
    Auto = 0x02,
    Neutral = 0x20,
    Weak = 0x21,
}
#[repr(C)]
pub struct Display {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarMode {
    Off,
    On,
    Active,
    Auto,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollSnap {
    None,
    Start,
    End,
    Center,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    HorizontalTop = 0x00,
    HorizontalBottom = 0x01,
    VerticalLeft = 0x02,
    VerticalRight = 0x04,
    RoundInner = 0x08,
    RoundOuter = 0x10,
    Last,
}
#[repr(C)]
pub struct ScaleSection {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarMode {
    Normal,
    Symmetrical,
    Range,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderMode {
    Normal = 0,
    Symmetrical = 1,
    Range = 2,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarOrientation {
    Auto,
    Horizontal,
    Vertical,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanOverflow {
    Clip,
    Ellipsis,
    Last,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanMode {
    Fixed,
    Expand,
    Break,
    Last,
}
#[repr(C)]
pub struct ChartCursor {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union StyleValue {
    pub num: i32,
    pub ptr: *const core::ffi::c_void,
    pub color: Color,
}
#[repr(C)]
pub struct Layer {
    _private: [u8; 0],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartUpdateMode {
    Shift,
    Circular,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartAxis {
    PrimaryY = 0x00,
    SecondaryY = 0x01,
    PrimaryX = 0x02,
    SecondaryX = 0x04,
    Last,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LvglResult {
    Invalid = 0,
    Ok,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleRes {
    NotFound,
    Found,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelLongMode {
    Wrap,
    Dot,
    Scroll,
    ScrollCircular,
    Clip,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenLoadAnim {
    None,
    OverLeft,
    OverRight,
    OverTop,
    OverBottom,
    MoveLeft,
    MoveRight,
    MoveTop,
    MoveBottom,
    FadeInOrOn,
    FadeOut,
    OutLeft,
    OutRight,
    OutTop,
    OutBottom,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderOrientation {
    Auto,
    Horizontal,
    Vertical,
}
#[repr(C)]
pub struct Matrix {
    pub m: [[f32; 3]; 3],
}
pub const COORD_TYPE_SHIFT: u32 = 29;
pub const COORD_TYPE_MASK: i32 = 3 << COORD_TYPE_SHIFT;
pub const COORD_TYPE_PX: i32 = 0 << COORD_TYPE_SHIFT;
pub const COORD_TYPE_SPEC: i32 = 1 << COORD_TYPE_SHIFT;
pub const COORD_TYPE_PX_NEG: i32 = 3 << COORD_TYPE_SHIFT;
pub const COORD_MAX: i32 = (1 << COORD_TYPE_SHIFT) - 1;
pub const COORD_MIN: i32 = -COORD_MAX;
#[inline]
pub fn coord_type(x: i32) -> i32 {
    x & COORD_TYPE_MASK
}
#[inline]
pub fn coord_plain(x: i32) -> i32 {
    x & !COORD_TYPE_MASK
}
#[inline]
pub fn coord_is_px(x: i32) -> bool {
    coord_type(x) == COORD_TYPE_PX || coord_type(x) == COORD_TYPE_PX_NEG
}
#[inline]
pub fn coord_is_spec(x: i32) -> bool {
    coord_type(x) == COORD_TYPE_SPEC
}
#[inline]
pub fn coord_set_spec(x: i32) -> i32 {
    x | COORD_TYPE_SPEC
}
#[inline]
pub fn max(a: i32, b: i32) -> i32 {
    if a > b { a } else { b }
}
#[inline]
pub fn min(a: i32, b: i32) -> i32 {
    if a < b { a } else { b }
}
#[inline]
pub fn pct(x: i32) -> i32 {
    coord_set_spec(x)
}
#[inline]
pub fn coord_is_pct(x: i32) -> bool {
    coord_is_spec(x)
}
#[inline]
pub fn coord_get_pct(x: i32) -> i32 {
    coord_plain(x)
}
#[inline]
pub fn size_content() -> i32 {
    coord_set_spec(COORD_MAX)
}
pub fn as_usize<T: Copy>(value: T) -> usize {
    assert!(size_of::< T > () <= size_of::< usize > ());
    let mut bytes = [0u8; size_of::<usize>()];
    let src = unsafe {
        core::slice::from_raw_parts(&value as *const _ as *const u8, size_of::<T>())
    };
    bytes[..size_of::<T>()].copy_from_slice(src);
    usize::from_ne_bytes(bytes)
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventCode {
    All = 0,
    Pressed,
    Pressing,
    PressLost,
    ShortClicked,
    SingleClicked,
    DoubleClicked,
    TripleClicked,
    LongPressed,
    LongPressedRepeat,
    Clicked,
    Released,
    ScrollBegin,
    ScrollThrowBegin,
    ScrollEnd,
    Scroll,
    Gesture,
    Key,
    Rotary,
    Focused,
    Defocused,
    Leave,
    HitTest,
    IndevReset,
    HoverOver,
    HoverLeave,
    CoverCheck,
    RefreshExtDrawSize,
    DrawMainBegin,
    DrawMain,
    DrawMainEnd,
    DrawPostBegin,
    DrawPost,
    DrawPostEnd,
    DrawTaskAdded,
    ValueChanged,
    Insert,
    Refresh,
    Ready,
    Cancel,
    Create,
    Delete,
    ChildChanged,
    ChildCreated,
    ChildDeleted,
    ScreenUnloadStart,
    ScreenLoadStart,
    ScreenLoaded,
    ScreenUnloaded,
    SizeChanged,
    StyleChanged,
    LayoutChanged,
    GetSelfSize,
    InvalidateArea,
    ResolutionChanged,
    ColorFormatChanged,
    RefreshRequest,
    RefreshStart,
    RefreshReady,
    RenderStart,
    RenderReady,
    FlushStart,
    FlushFinish,
    FlushWaitStart,
    FlushWaitFinish,
    Vsync,
    VsyncRequest,
    Last,
    Preprocess = 0x8000,
    MarkedDeleting = 0x10000,
}
/// Xila window close request, separate from LVGL's standard events.
pub const EVENT_CLOSE_REQUESTED: u32 = 70;
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
#[repr(u16)]
pub enum FunctionCall {
    WindowCreate = 1037,
    FilePickerCreate = 61485,
    FilePickerReset = 61488,
    FilePickerGetStatus = 61486,
    FilePickerGetPath = 61487,
    WindowSetIcon = 1042,
    WindowPopEvent = 1041,
    Percentage = 1034,
    ObjectCreate = 248,
    ObjectDelete = 1033,
    ObjectSetFlexFlow = 554,
    ObjectSetFlexGrow = 555,
    ObjectSetHeight = 564,
    ObjectSetWidth = 736,
    ObjectSetSize = 591,
    ObjectSetPos = 572,
    ObjectGetX = 443,
    ObjectGetY = 446,
    ObjectGetWidth = 442,
    ObjectGetHeight = 266,
    ObjectAlign = 236,
    ObjectUpdateLayout = 753,
    ObjectSetFlexAlign = 553,
    ObjectSetLayout = 568,
    ObjectSetGridDscArray = 563,
    ObjectSetGridAlign = 561,
    ObjectSetGridCell = 562,
    GridFr = 61446,
    ObjectAddState = 234,
    ObjectGetState = 288,
    ButtonCreate = 28,
    CheckboxCreate = 100,
    CheckboxSetText = 102,
    TabviewCreate = 980,
    TabviewAddTab = 979,
    TabviewGetTabActive = 982,
    ObjectAddFlag = 232,
    LabelCreate = 160,
    LabelSetText = 175,
    ButtonmatrixCreate = 31,
    ButtonmatrixSetMap = 40,
    ButtonmatrixGetSelectedButton = 34,
    ArcCreate = 1,
    SliderCreate = 776,
    ObjectRemoveState = 520,
    DropdownCreate = 139,
    DropdownSetOptions = 153,
    DropdownSetSelected = 155,
    DropdownGetSelected = 145,
    DropdownGetOptionCount = 142,
    DropdownSetSelectedHighlight = 156,
    DropdownOpen = 151,
    DropdownClose = 138,
    RollerCreate = 763,
    RollerSetOptions = 769,
    RollerSetSelected = 770,
    RollerGetSelected = 767,
    RollerGetOptionCount = 764,
    RollerSetVisibleRowCount = 772,
    CalendarCreate = 45,
    CalendarSetMonthShown = 55,
    SpinboxCreate = 792,
    SpinboxSetRange = 810,
    SpinboxSetValue = 813,
    SpinboxGetValue = 801,
    SpinboxSetStep = 812,
    SpinboxSetDigitFormat = 806,
    SpinboxSetRollover = 811,
    SpinboxIncrement = 802,
    SpinboxDecrement = 793,
    ChartCreate = 63,
    ChartSetType = 96,
    ChartSetPointCount = 88,
    ChartSetAxisRange = 83,
    LineCreate = 188,
    LineSetPoints = 194,
    LineSetYInvert = 196,
    TileviewCreate = 1029,
    TileviewAddTile = 1028,
    TileviewSetTile = 1031,
    TileviewGetTileActive = 1030,
    ObjectSetPadding = 61445,
    IndexedCanvasCreate = 61440,
    IndexedCanvasSetPixels = 61441,
    WindowPopPointerEvent = 61442,
    TextareaCreate = 994,
    TextareaSetOneLine = 1020,
    TextareaSetText = 1025,
    TextareaSetMaxLength = 1019,
    TextareaGetText = 1035,
    TextareaGetTextLength = 1036,
    KeyboardCreate = 61443,
    KeyboardSetTextarea = 61444,
    ObjectRemoveFlag = 518,
    ObjectSetBackground = 61447,
    ObjectSetTextColor = 61448,
    ObjectSetBorder = 61449,
    ObjectSetRadius = 61450,
    ObjectSetTextAlign = 61451,
    ObjectSetFont = 61452,
    ObjectSetSurface = 61453,
    ObjectSetMinMaxSize = 61454,
    BarCreate = 61455,
    SwitchCreate = 61456,
    SpinnerCreate = 61457,
    LedCreate = 180,
    LedSetBrightness = 185,
    LedOn = 184,
    LedOff = 183,
    LedToggle = 187,
    RangeSetRange = 61458,
    RangeSetValue = 61459,
    RangeGetValue = 61460,
    RangeGetStartValue = 61461,
    RangeSetMode = 61462,
    RangeSetOrientation = 61463,
    SpinnerSetAnimation = 61464,
    SpinnerGetAnimationDuration = 61465,
    SpinnerGetArcSweep = 61466,
    DropdownGetSelectedString = 61467,
    RollerGetSelectedString = 61468,
    DropdownGetHighlight = 61469,
    DropdownIsOpenState = 61470,
    CalendarSetToday = 61471,
    CalendarSetHighlights = 61472,
    CalendarGetShownDate = 61473,
    ChartAddSeriesSlot = 61474,
    ChartSetValues = 61475,
    ChartGetValue = 61476,
    ListAddTextButton = 61479,
    ListCreate = 61477,
    ListAddText = 61478,
    ListSetButtonText = 61480,
    ObjectSetThemeVariant = 61481,
    ObjectSetThemeError = 61482,
    BadgeCreate = 61483,
    CardCreate = 61484,
}
pub type TextLeadingTrim = core::ffi::c_uint;
pub type BlurQuality = core::ffi::c_uint;
pub type State = core::ffi::c_uint;
pub const STATE_DEFAULT: State = 0;
pub const STATE_ALT: State = 1;
pub const STATE_CHECKED: State = 4;
pub const STATE_FOCUSED: State = 8;
pub const STATE_FOCUS_KEY: State = 16;
pub const STATE_EDITED: State = 32;
pub const STATE_HOVERED: State = 64;
pub const STATE_PRESSED: State = 128;
pub const STATE_SCROLLED: State = 256;
pub const STATE_DISABLED: State = 512;
pub const STATE_USER_1: State = 4096;
pub const STATE_USER_2: State = 8192;
pub const STATE_USER_3: State = 16384;
pub const STATE_USER_4: State = 32768;
pub const STATE_ANY: State = 65535;
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn window_create() -> Result<*mut Object> {
    unsafe { call(FunctionCall::WindowCreate, &[]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn file_picker_create(
    parent: *mut Object,
    mode: u32,
    initial: *const core::ffi::c_char,
) -> Result<*mut Object> {
    unsafe {
        call(
            FunctionCall::FilePickerCreate,
            &[parent as usize, mode as usize, initial as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn file_picker_reset(
    picker: *mut Object,
    mode: u32,
    initial: *const core::ffi::c_char,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::FilePickerReset,
            &[picker as usize, mode as usize, initial as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn file_picker_get_status(picker: *mut Object) -> Result<u32> {
    unsafe { call(FunctionCall::FilePickerGetStatus, &[picker as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn file_picker_get_path(
    picker: *mut Object,
    buffer: *mut i8,
    size: usize,
) -> Result<i32> {
    unsafe {
        call(FunctionCall::FilePickerGetPath, &[picker as usize, buffer as usize, size])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn window_set_icon(
    window: *mut Object,
    icon_string: *const core::ffi::c_char,
    icon_color: Color,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::WindowSetIcon,
            &[window as usize, icon_string as usize, as_usize(icon_color)],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn window_pop_event(
    window: *mut Object,
    code: *mut u32,
    target: *mut c_void,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::WindowPopEvent,
            &[window as usize, code as usize, target as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn percentage(value: i32) -> Result<i32> {
    unsafe { call(FunctionCall::Percentage, &[value as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ObjectCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_delete(object: u32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectDelete, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_flex_flow(obj: *mut Object, flow: FlexFlow) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetFlexFlow, &[obj as usize, flow as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_flex_grow(obj: *mut Object, grow: u8) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetFlexGrow, &[obj as usize, grow as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_height(obj: *mut Object, h: i32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetHeight, &[obj as usize, h as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_width(obj: *mut Object, w: i32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetWidth, &[obj as usize, w as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_size(obj: *mut Object, w: i32, h: i32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetSize, &[obj as usize, w as usize, h as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_pos(obj: *mut Object, x: i32, y: i32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetPos, &[obj as usize, x as usize, y as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_get_x(obj: *const Object) -> Result<i32> {
    unsafe { call(FunctionCall::ObjectGetX, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_get_y(obj: *const Object) -> Result<i32> {
    unsafe { call(FunctionCall::ObjectGetY, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_get_width(obj: *const Object) -> Result<i32> {
    unsafe { call(FunctionCall::ObjectGetWidth, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_get_height(obj: *const Object) -> Result<i32> {
    unsafe { call(FunctionCall::ObjectGetHeight, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_align(
    obj: *mut Object,
    align: Align,
    x_ofs: i32,
    y_ofs: i32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectAlign,
            &[obj as usize, align as usize, x_ofs as usize, y_ofs as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_update_layout(obj: *const Object) -> Result<()> {
    unsafe { call(FunctionCall::ObjectUpdateLayout, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_flex_align(
    obj: *mut Object,
    main_place: FlexAlign,
    cross_place: FlexAlign,
    track_cross_place: FlexAlign,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetFlexAlign,
            &[
                obj as usize,
                main_place as usize,
                cross_place as usize,
                track_cross_place as usize,
            ],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_layout(obj: *mut Object, layout: u32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetLayout, &[obj as usize, layout as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_grid_dsc_array(
    obj: *mut Object,
    col_dsc: *const i32,
    row_dsc: *const i32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetGridDscArray,
            &[obj as usize, col_dsc as usize, row_dsc as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_grid_align(
    obj: *mut Object,
    column_align: GridAlign,
    row_align: GridAlign,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetGridAlign,
            &[obj as usize, column_align as usize, row_align as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_grid_cell(
    obj: *mut Object,
    column_align: GridAlign,
    col_pos: i32,
    col_span: i32,
    row_align: GridAlign,
    row_pos: i32,
    row_span: i32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetGridCell,
            &[
                obj as usize,
                column_align as usize,
                col_pos as usize,
                col_span as usize,
                row_align as usize,
                row_pos as usize,
                row_span as usize,
            ],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn grid_fr(weight: u8) -> Result<i32> {
    unsafe { call(FunctionCall::GridFr, &[weight as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_add_state(obj: *mut Object, state: State) -> Result<()> {
    unsafe { call(FunctionCall::ObjectAddState, &[obj as usize, state as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_get_state(obj: *const Object) -> Result<State> {
    unsafe { call(FunctionCall::ObjectGetState, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn button_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ButtonCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn checkbox_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::CheckboxCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn checkbox_set_text(
    obj: *mut Object,
    txt: *const core::ffi::c_char,
) -> Result<()> {
    unsafe { call(FunctionCall::CheckboxSetText, &[obj as usize, txt as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tabview_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::TabviewCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tabview_add_tab(
    obj: *mut Object,
    name: *const core::ffi::c_char,
) -> Result<*mut Object> {
    unsafe { call(FunctionCall::TabviewAddTab, &[obj as usize, name as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tabview_get_tab_active(obj: *mut Object) -> Result<u32> {
    unsafe { call(FunctionCall::TabviewGetTabActive, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_add_flag(obj: *mut Object, f: ObjectFlag) -> Result<()> {
    unsafe { call(FunctionCall::ObjectAddFlag, &[obj as usize, f as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn label_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::LabelCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn label_set_text(
    obj: *mut Object,
    text: *const core::ffi::c_char,
) -> Result<()> {
    unsafe { call(FunctionCall::LabelSetText, &[obj as usize, text as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn buttonmatrix_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ButtonmatrixCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn buttonmatrix_set_map(
    obj: *mut Object,
    map: *const *const core::ffi::c_char,
) -> Result<()> {
    unsafe { call(FunctionCall::ButtonmatrixSetMap, &[obj as usize, map as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn buttonmatrix_get_selected_button(obj: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::ButtonmatrixGetSelectedButton, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn arc_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ArcCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn slider_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::SliderCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_remove_state(obj: *mut Object, state: State) -> Result<()> {
    unsafe { call(FunctionCall::ObjectRemoveState, &[obj as usize, state as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::DropdownCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_set_options(
    obj: *mut Object,
    options: *const core::ffi::c_char,
) -> Result<()> {
    unsafe { call(FunctionCall::DropdownSetOptions, &[obj as usize, options as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_set_selected(obj: *mut Object, sel_opt: u32) -> Result<()> {
    unsafe { call(FunctionCall::DropdownSetSelected, &[obj as usize, sel_opt as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_get_selected(obj: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::DropdownGetSelected, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_get_option_count(obj: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::DropdownGetOptionCount, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_set_selected_highlight(obj: *mut Object, en: bool) -> Result<()> {
    unsafe {
        call(FunctionCall::DropdownSetSelectedHighlight, &[obj as usize, en as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_open(dropdown_obj: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::DropdownOpen, &[dropdown_obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_close(obj: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::DropdownClose, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::RollerCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_set_options(
    obj: *mut Object,
    options: *const core::ffi::c_char,
    mode: RollerMode,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::RollerSetOptions,
            &[obj as usize, options as usize, mode as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_set_selected(
    obj: *mut Object,
    sel_opt: u32,
    anim: AnimEnable,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::RollerSetSelected,
            &[obj as usize, sel_opt as usize, anim as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_get_selected(obj: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::RollerGetSelected, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_get_option_count(obj: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::RollerGetOptionCount, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_set_visible_row_count(
    obj: *mut Object,
    row_cnt: u32,
) -> Result<()> {
    unsafe {
        call(FunctionCall::RollerSetVisibleRowCount, &[obj as usize, row_cnt as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn calendar_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::CalendarCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn calendar_set_month_shown(
    obj: *mut Object,
    year: u32,
    month: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::CalendarSetMonthShown,
            &[obj as usize, year as usize, month as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::SpinboxCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_set_range(
    obj: *mut Object,
    min_value: i32,
    max_value: i32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::SpinboxSetRange,
            &[obj as usize, min_value as usize, max_value as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_set_value(obj: *mut Object, v: i32) -> Result<()> {
    unsafe { call(FunctionCall::SpinboxSetValue, &[obj as usize, v as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_get_value(obj: *mut Object) -> Result<i32> {
    unsafe { call(FunctionCall::SpinboxGetValue, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_set_step(obj: *mut Object, step: u32) -> Result<()> {
    unsafe { call(FunctionCall::SpinboxSetStep, &[obj as usize, step as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_set_digit_format(
    obj: *mut Object,
    digit_count: u32,
    sep_pos: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::SpinboxSetDigitFormat,
            &[obj as usize, digit_count as usize, sep_pos as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_set_rollover(obj: *mut Object, rollover: bool) -> Result<()> {
    unsafe { call(FunctionCall::SpinboxSetRollover, &[obj as usize, rollover as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_increment(obj: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::SpinboxIncrement, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinbox_decrement(obj: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::SpinboxDecrement, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ChartCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_set_type(obj: *mut Object, type_: ChartType) -> Result<()> {
    unsafe { call(FunctionCall::ChartSetType, &[obj as usize, type_ as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_set_point_count(obj: *mut Object, cnt: u32) -> Result<()> {
    unsafe { call(FunctionCall::ChartSetPointCount, &[obj as usize, cnt as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_set_axis_range(
    obj: *mut Object,
    axis: ChartAxis,
    min: i32,
    max: i32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ChartSetAxisRange,
            &[obj as usize, axis as usize, min as usize, max as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn line_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::LineCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn line_set_points(
    obj: *mut Object,
    points: *const PointPrecise,
    point_num: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::LineSetPoints,
            &[obj as usize, points as usize, point_num as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn line_set_y_invert(obj: *mut Object, en: bool) -> Result<()> {
    unsafe { call(FunctionCall::LineSetYInvert, &[obj as usize, en as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tileview_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::TileviewCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tileview_add_tile(
    tv: *mut Object,
    col_id: u8,
    row_id: u8,
    dir: Dir,
) -> Result<*mut Object> {
    unsafe {
        call(
            FunctionCall::TileviewAddTile,
            &[tv as usize, col_id as usize, row_id as usize, dir as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tileview_set_tile(
    obj: *mut Object,
    tile_obj: *mut Object,
    anim_en: AnimEnable,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::TileviewSetTile,
            &[obj as usize, tile_obj as usize, anim_en as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn tileview_get_tile_active(obj: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::TileviewGetTileActive, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_padding(object: *mut Object, padding: i32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetPadding, &[object as usize, padding as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn indexed_canvas_create(
    parent: *mut Object,
    width: u16,
    height: u16,
) -> Result<*mut Object> {
    unsafe {
        call(
            FunctionCall::IndexedCanvasCreate,
            &[parent as usize, width as usize, height as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn indexed_canvas_set_pixels(
    canvas: *mut Object,
    pixels: *const u8,
    length: usize,
    palette: *const u32,
    colors: usize,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::IndexedCanvasSetPixels,
            &[canvas as usize, pixels as usize, length, palette as usize, colors],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn window_pop_pointer_event(
    window: *mut Object,
    code: *mut u32,
    target: *mut c_void,
    x: *mut i32,
    y: *mut i32,
    width: *mut i32,
    height: *mut i32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::WindowPopPointerEvent,
            &[
                window as usize,
                code as usize,
                target as usize,
                x as usize,
                y as usize,
                width as usize,
                height as usize,
            ],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn textarea_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::TextareaCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn textarea_set_one_line(obj: *mut Object, en: bool) -> Result<()> {
    unsafe { call(FunctionCall::TextareaSetOneLine, &[obj as usize, en as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn textarea_set_text(
    obj: *mut Object,
    txt: *const core::ffi::c_char,
) -> Result<()> {
    unsafe { call(FunctionCall::TextareaSetText, &[obj as usize, txt as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn textarea_set_max_length(obj: *mut Object, num: u32) -> Result<()> {
    unsafe { call(FunctionCall::TextareaSetMaxLength, &[obj as usize, num as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn textarea_get_text(
    textarea: *mut Object,
    buffer: *mut i8,
    buffer_size: usize,
) -> Result<i32> {
    unsafe {
        call(
            FunctionCall::TextareaGetText,
            &[textarea as usize, buffer as usize, buffer_size],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn textarea_get_text_length(textarea: *mut Object) -> Result<i32> {
    unsafe { call(FunctionCall::TextareaGetTextLength, &[textarea as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn keyboard_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::KeyboardCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn keyboard_set_textarea(
    keyboard: *mut Object,
    textarea: *mut Object,
) -> Result<()> {
    unsafe {
        call(FunctionCall::KeyboardSetTextarea, &[keyboard as usize, textarea as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_remove_flag(obj: *mut Object, f: ObjectFlag) -> Result<()> {
    unsafe { call(FunctionCall::ObjectRemoveFlag, &[obj as usize, f as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_background(
    object: *mut Object,
    rgb: u32,
    opacity: u8,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetBackground,
            &[object as usize, rgb as usize, opacity as usize, selector as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_text_color(
    object: *mut Object,
    rgb: u32,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetTextColor,
            &[object as usize, rgb as usize, selector as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_border(
    object: *mut Object,
    rgb: u32,
    width: i32,
    side: u32,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetBorder,
            &[
                object as usize,
                rgb as usize,
                width as usize,
                side as usize,
                selector as usize,
            ],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_radius(
    object: *mut Object,
    radius: i32,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetRadius,
            &[object as usize, radius as usize, selector as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_text_align(
    object: *mut Object,
    align: u32,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetTextAlign,
            &[object as usize, align as usize, selector as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_font(
    object: *mut Object,
    font: u32,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetFont,
            &[object as usize, font as usize, selector as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_surface(object: *mut Object, surface: u32) -> Result<()> {
    unsafe { call(FunctionCall::ObjectSetSurface, &[object as usize, surface as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_min_max_size(
    object: *mut Object,
    min_width: i32,
    max_width: i32,
    min_height: i32,
    max_height: i32,
    selector: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ObjectSetMinMaxSize,
            &[
                object as usize,
                min_width as usize,
                max_width as usize,
                min_height as usize,
                max_height as usize,
                selector as usize,
            ],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn bar_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::BarCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn switch_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::SwitchCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinner_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::SpinnerCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn led_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::LedCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn led_set_brightness(obj: *mut Object, bright: u8) -> Result<()> {
    unsafe { call(FunctionCall::LedSetBrightness, &[obj as usize, bright as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn led_on(led: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::LedOn, &[led as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn led_off(led: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::LedOff, &[led as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn led_toggle(obj: *mut Object) -> Result<()> {
    unsafe { call(FunctionCall::LedToggle, &[obj as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn range_set_range(object: *mut Object, min: i32, max: i32) -> Result<()> {
    unsafe {
        call(FunctionCall::RangeSetRange, &[object as usize, min as usize, max as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn range_set_value(
    object: *mut Object,
    value: i32,
    animated: bool,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::RangeSetValue,
            &[object as usize, value as usize, animated as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn range_get_value(object: *const Object) -> Result<i32> {
    unsafe { call(FunctionCall::RangeGetValue, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn range_get_start_value(object: *const Object) -> Result<i32> {
    unsafe { call(FunctionCall::RangeGetStartValue, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn range_set_mode(object: *mut Object, mode: u32) -> Result<()> {
    unsafe { call(FunctionCall::RangeSetMode, &[object as usize, mode as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn range_set_orientation(
    object: *mut Object,
    orientation: u32,
) -> Result<()> {
    unsafe {
        call(FunctionCall::RangeSetOrientation, &[object as usize, orientation as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinner_set_animation(
    object: *mut Object,
    duration_ms: u32,
    sweep_degrees: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::SpinnerSetAnimation,
            &[object as usize, duration_ms as usize, sweep_degrees as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinner_get_animation_duration(object: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::SpinnerGetAnimationDuration, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn spinner_get_arc_sweep(object: *const Object) -> Result<u32> {
    unsafe { call(FunctionCall::SpinnerGetArcSweep, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_get_selected_string(
    object: *mut Object,
    output: *mut i8,
    capacity: u32,
) -> Result<i32> {
    unsafe {
        call(
            FunctionCall::DropdownGetSelectedString,
            &[object as usize, output as usize, capacity as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn roller_get_selected_string(
    object: *mut Object,
    output: *mut i8,
    capacity: u32,
) -> Result<i32> {
    unsafe {
        call(
            FunctionCall::RollerGetSelectedString,
            &[object as usize, output as usize, capacity as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_get_highlight(object: *mut Object) -> Result<u32> {
    unsafe { call(FunctionCall::DropdownGetHighlight, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn dropdown_is_open_state(object: *mut Object) -> Result<u32> {
    unsafe { call(FunctionCall::DropdownIsOpenState, &[object as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn calendar_set_today(
    calendar: *mut Object,
    year: u32,
    month: u32,
    day: u32,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::CalendarSetToday,
            &[calendar as usize, year as usize, month as usize, day as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn calendar_set_highlights(
    calendar: *mut Object,
    dates: *const u8,
    count: usize,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::CalendarSetHighlights,
            &[calendar as usize, dates as usize, count],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn calendar_get_shown_date(
    calendar: *mut Object,
    output: *mut u8,
) -> Result<()> {
    unsafe {
        call(FunctionCall::CalendarGetShownDate, &[calendar as usize, output as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_add_series_slot(chart: *mut Object, rgb: u32) -> Result<u32> {
    unsafe { call(FunctionCall::ChartAddSeriesSlot, &[chart as usize, rgb as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_set_values(
    chart: *mut Object,
    series_slot: u32,
    values: *const i32,
    count: usize,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ChartSetValues,
            &[chart as usize, series_slot as usize, values as usize, count],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn chart_get_value(
    chart: *mut Object,
    series_slot: u32,
    index: u32,
) -> Result<i32> {
    unsafe {
        call(
            FunctionCall::ChartGetValue,
            &[chart as usize, series_slot as usize, index as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn list_add_text_button(
    list: *mut Object,
    text: *const core::ffi::c_char,
) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ListAddTextButton, &[list as usize, text as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn list_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ListCreate, &[parent as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn list_add_text(
    list: *mut Object,
    text: *const core::ffi::c_char,
) -> Result<*mut Object> {
    unsafe { call(FunctionCall::ListAddText, &[list as usize, text as usize]) }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn list_set_button_text(
    list: *mut Object,
    button: *mut Object,
    text: *const core::ffi::c_char,
) -> Result<()> {
    unsafe {
        call(
            FunctionCall::ListSetButtonText,
            &[list as usize, button as usize, text as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_theme_variant(object: *mut Object, variant: u32) -> Result<()> {
    unsafe {
        call(FunctionCall::ObjectSetThemeVariant, &[object as usize, variant as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn object_set_theme_error(object: *mut Object, error: bool) -> Result<()> {
    unsafe {
        call(FunctionCall::ObjectSetThemeError, &[object as usize, error as usize])
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn badge_create(
    parent: *mut Object,
    text: *const core::ffi::c_char,
    variant: u32,
) -> Result<*mut Object> {
    unsafe {
        call(
            FunctionCall::BadgeCreate,
            &[parent as usize, text as usize, variant as usize],
        )
    }
}
/// # Safety
/// Pointers must be valid for the call, and the call must run in a native process.
pub unsafe fn card_create(parent: *mut Object) -> Result<*mut Object> {
    unsafe { call(FunctionCall::CardCreate, &[parent as usize]) }
}
