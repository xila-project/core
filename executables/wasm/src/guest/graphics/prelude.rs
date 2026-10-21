pub use crate::FunctionCall;
pub use crate::{EVENT_CLOSE_REQUESTED, EventCode};
pub use core::ffi::c_void;

#[link(wasm_import_module = "host")]
unsafe extern "C" {
    #[link_name = "xila_graphics_call"]
    pub fn xila_graphics_call(
        function: FunctionCall,
        argument_0: usize,
        argument_1: usize,
        argument_2: usize,
        argument_3: usize,
        argument_4: usize,
        argument_5: usize,
        argument_6: usize,
        argument_count: u8,
        return_value: *mut core::ffi::c_void,
    ) -> i32;
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
    ColumnWrapReverse = FLEX_COLUMN as isize | FLEX_WRAP as isize | FLEX_REVERSE as isize,
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
pub type ObjectHandle = u16;
pub type LvObject = u16;
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
pub enum ChartType {
    None,
    Line,
    Bar,
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

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeVariant {
    Default = 0,
    Secondary = 1,
    Outline = 2,
    Ghost = 3,
    Destructive = 4,
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

// Coordinate type constants
pub const COORD_TYPE_SHIFT: u32 = 29;
pub const COORD_TYPE_MASK: i32 = 3 << COORD_TYPE_SHIFT;
pub const COORD_TYPE_PX: i32 = 0 << COORD_TYPE_SHIFT;
pub const COORD_TYPE_SPEC: i32 = 1 << COORD_TYPE_SHIFT;
pub const COORD_TYPE_PX_NEG: i32 = 3 << COORD_TYPE_SHIFT;
pub const COORD_MAX: i32 = (1 << COORD_TYPE_SHIFT) - 1;
pub const COORD_MIN: i32 = -COORD_MAX;

// Helper functions for coordinates
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
    assert!(size_of::<T>() <= size_of::<usize>());

    let mut bytes = [0u8; size_of::<usize>()];
    let src =
        unsafe { core::slice::from_raw_parts(&value as *const _ as *const u8, size_of::<T>()) };
    bytes[..size_of::<T>()].copy_from_slice(src);
    usize::from_ne_bytes(bytes)
}

pub use super::enumeration::{
    BlurQuality, STATE_ANY, STATE_CHECKED, STATE_DEFAULT, STATE_DISABLED, STATE_EDITED,
    STATE_FOCUS_KEY, STATE_FOCUSED, STATE_HOVERED, STATE_PRESSED, STATE_SCROLLED, STATE_USER_1,
    STATE_USER_2, STATE_USER_3, STATE_USER_4, State, TextLeadingTrim,
};
