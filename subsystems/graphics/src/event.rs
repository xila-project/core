use crate::{Key, ffi};

#[derive(Debug, Clone)]
pub struct Event {
    pub code: EventKind,
    pub(super) target: *mut ffi::lv_obj_t,
    /// The target's identifier (see [`crate::object_id`]) when the event was
    /// queued, zero if it had none.
    pub(super) target_id: u32,
    pub key: Option<Key>,
    /// Pointer position relative to the target content, plus content width/height,
    /// captured at event time rather than when a consumer drains the queue.
    pub pointer: Option<[i32; 4]>,
}

impl Event {
    /// Compare the queued address with a candidate widget. The event carries
    /// no lifetime guarantee, and LVGL may reuse an address after deletion.
    pub fn is_target(&self, candidate: *const ffi::lv_obj_t) -> bool {
        core::ptr::eq(self.target, candidate)
    }

    /// The identifier the target carried when the event was queued, if any.
    /// Resolve it through the consumer's
    /// [`ObjectTable`](crate::object_id::ObjectTable): it names nothing once
    /// the target is deleted, and is never reissued, unlike the address.
    pub fn target_id(&self) -> Option<core::num::NonZeroU32> {
        core::num::NonZeroU32::new(self.target_id)
    }

    pub fn new(code: EventKind, target: *mut ffi::lv_obj_t, key: Option<Key>) -> Self {
        Self {
            code,
            target,
            target_id: 0,
            key,
            pointer: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum EventKind {
    All = ffi::lv_event_code_t_LV_EVENT_ALL as u16,
    Pressed = ffi::lv_event_code_t_LV_EVENT_PRESSED as u16,
    Pressing = ffi::lv_event_code_t_LV_EVENT_PRESSING as u16,
    PressLost = ffi::lv_event_code_t_LV_EVENT_PRESS_LOST as u16,
    ShortClicked = ffi::lv_event_code_t_LV_EVENT_SHORT_CLICKED as u16,
    SingleClicked = ffi::lv_event_code_t_LV_EVENT_SINGLE_CLICKED as u16,
    DoubleClicked = ffi::lv_event_code_t_LV_EVENT_DOUBLE_CLICKED as u16,
    TripleClicked = ffi::lv_event_code_t_LV_EVENT_TRIPLE_CLICKED as u16,
    LongPressed = ffi::lv_event_code_t_LV_EVENT_LONG_PRESSED as u16,
    LongPressedRepeat = ffi::lv_event_code_t_LV_EVENT_LONG_PRESSED_REPEAT as u16,
    Clicked = ffi::lv_event_code_t_LV_EVENT_CLICKED as u16,
    Released = ffi::lv_event_code_t_LV_EVENT_RELEASED as u16,
    ScrollBegin = ffi::lv_event_code_t_LV_EVENT_SCROLL_BEGIN as u16,
    ScrollThrowBegin = ffi::lv_event_code_t_LV_EVENT_SCROLL_THROW_BEGIN as u16,
    ScrollEnd = ffi::lv_event_code_t_LV_EVENT_SCROLL_END as u16,
    Scroll = ffi::lv_event_code_t_LV_EVENT_SCROLL as u16,
    Gesture = ffi::lv_event_code_t_LV_EVENT_GESTURE as u16,
    Key = ffi::lv_event_code_t_LV_EVENT_KEY as u16,
    Rotary = ffi::lv_event_code_t_LV_EVENT_ROTARY as u16,
    Focused = ffi::lv_event_code_t_LV_EVENT_FOCUSED as u16,
    Defocused = ffi::lv_event_code_t_LV_EVENT_DEFOCUSED as u16,
    Leave = ffi::lv_event_code_t_LV_EVENT_LEAVE as u16,
    HitTest = ffi::lv_event_code_t_LV_EVENT_HIT_TEST as u16,
    InputDeviceReset = ffi::lv_event_code_t_LV_EVENT_INDEV_RESET as u16,
    HoverOver = ffi::lv_event_code_t_LV_EVENT_HOVER_OVER as u16,
    HoverLeave = ffi::lv_event_code_t_LV_EVENT_HOVER_LEAVE as u16,
    CoverCheck = ffi::lv_event_code_t_LV_EVENT_COVER_CHECK as u16,
    RefreshExtDrawSize = ffi::lv_event_code_t_LV_EVENT_REFR_EXT_DRAW_SIZE as u16,
    DrawMainBegin = ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN_BEGIN as u16,
    DrawMain = ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN as u16,
    DrawMainEnd = ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN_END as u16,
    DrawPostBegin = ffi::lv_event_code_t_LV_EVENT_DRAW_POST_BEGIN as u16,
    DrawPost = ffi::lv_event_code_t_LV_EVENT_DRAW_POST as u16,
    DrawPostEnd = ffi::lv_event_code_t_LV_EVENT_DRAW_POST_END as u16,
    DrawTaskAdded = ffi::lv_event_code_t_LV_EVENT_DRAW_TASK_ADDED as u16,
    ValueChanged = ffi::lv_event_code_t_LV_EVENT_VALUE_CHANGED as u16,
    Insert = ffi::lv_event_code_t_LV_EVENT_INSERT as u16,
    Refresh = ffi::lv_event_code_t_LV_EVENT_REFRESH as u16,
    Ready = ffi::lv_event_code_t_LV_EVENT_READY as u16,
    Cancel = ffi::lv_event_code_t_LV_EVENT_CANCEL as u16,
    Create = ffi::lv_event_code_t_LV_EVENT_CREATE as u16,
    Delete = ffi::lv_event_code_t_LV_EVENT_DELETE as u16,
    ChildChanged = ffi::lv_event_code_t_LV_EVENT_CHILD_CHANGED as u16,
    ChildCreated = ffi::lv_event_code_t_LV_EVENT_CHILD_CREATED as u16,
    ChildDeleted = ffi::lv_event_code_t_LV_EVENT_CHILD_DELETED as u16,
    ScreenUnloadStart = ffi::lv_event_code_t_LV_EVENT_SCREEN_UNLOAD_START as u16,
    ScreenLoadStart = ffi::lv_event_code_t_LV_EVENT_SCREEN_LOAD_START as u16,
    ScreenLoaded = ffi::lv_event_code_t_LV_EVENT_SCREEN_LOADED as u16,
    ScreenUnloaded = ffi::lv_event_code_t_LV_EVENT_SCREEN_UNLOADED as u16,
    SizeChanged = ffi::lv_event_code_t_LV_EVENT_SIZE_CHANGED as u16,
    StyleChanged = ffi::lv_event_code_t_LV_EVENT_STYLE_CHANGED as u16,
    LayoutChanged = ffi::lv_event_code_t_LV_EVENT_LAYOUT_CHANGED as u16,
    GetSelfSize = ffi::lv_event_code_t_LV_EVENT_GET_SELF_SIZE as u16,
    InvalidateArea = ffi::lv_event_code_t_LV_EVENT_INVALIDATE_AREA as u16,
    ResolutionChanged = ffi::lv_event_code_t_LV_EVENT_RESOLUTION_CHANGED as u16,
    ColorFormatChanged = ffi::lv_event_code_t_LV_EVENT_COLOR_FORMAT_CHANGED as u16,
    RefreshRequest = ffi::lv_event_code_t_LV_EVENT_REFR_REQUEST as u16,
    RefreshStart = ffi::lv_event_code_t_LV_EVENT_REFR_START as u16,
    RefreshReady = ffi::lv_event_code_t_LV_EVENT_REFR_READY as u16,
    RenderStart = ffi::lv_event_code_t_LV_EVENT_RENDER_START as u16,
    RenderReady = ffi::lv_event_code_t_LV_EVENT_RENDER_READY as u16,
    FlushStart = ffi::lv_event_code_t_LV_EVENT_FLUSH_START as u16,
    FlushFinish = ffi::lv_event_code_t_LV_EVENT_FLUSH_FINISH as u16,
    FlushWaitStart = ffi::lv_event_code_t_LV_EVENT_FLUSH_WAIT_START as u16,
    FlushWaitFinish = ffi::lv_event_code_t_LV_EVENT_FLUSH_WAIT_FINISH as u16,
    VerticalSynchronization = ffi::lv_event_code_t_LV_EVENT_VSYNC as u16,
    Last = ffi::lv_event_code_t_LV_EVENT_LAST as u16,
    Custom1 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 1,
    Custom2 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 2,
    CloseRequested = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 3,
    Custom4 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 4,
    Custom5 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 5,
    Custom6 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 6,
    Custom7 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 7,
    Custom8 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 8,
    Custom9 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 9,
    Custom10 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 10,
    Custom11 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 11,
    Custom12 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 12,
    Custom13 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 13,
    Custom14 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 14,
    Custom15 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 15,
    Custom16 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 16,
    Custom17 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 17,
    Custom18 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 18,
    Custom19 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 19,
    Custom20 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 20,
    Custom21 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 21,
    Custom22 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 22,
    Custom23 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 23,
    Custom24 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 24,
    Custom25 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 25,
    Custom26 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 26,
    Custom27 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 27,
    Custom28 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 28,
    Custom29 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 29,
    Custom30 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 30,
    Custom31 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 31,
    Custom32 = ffi::lv_event_code_t_LV_EVENT_LAST as u16 + 32,
    Preprocess = 0x8000,
}

impl EventKind {
    pub const fn into_lvgl_code(self) -> ffi::lv_event_code_t {
        self as ffi::lv_event_code_t
    }

    pub const fn from_lvgl_code(code: ffi::lv_event_code_t) -> Self {
        match code {
            ffi::lv_event_code_t_LV_EVENT_ALL => Self::All,
            ffi::lv_event_code_t_LV_EVENT_PRESSED => Self::Pressed,
            ffi::lv_event_code_t_LV_EVENT_PRESSING => Self::Pressing,
            ffi::lv_event_code_t_LV_EVENT_PRESS_LOST => Self::PressLost,
            ffi::lv_event_code_t_LV_EVENT_SHORT_CLICKED => Self::ShortClicked,
            ffi::lv_event_code_t_LV_EVENT_SINGLE_CLICKED => Self::SingleClicked,
            ffi::lv_event_code_t_LV_EVENT_DOUBLE_CLICKED => Self::DoubleClicked,
            ffi::lv_event_code_t_LV_EVENT_TRIPLE_CLICKED => Self::TripleClicked,
            ffi::lv_event_code_t_LV_EVENT_LONG_PRESSED => Self::LongPressed,
            ffi::lv_event_code_t_LV_EVENT_LONG_PRESSED_REPEAT => Self::LongPressedRepeat,
            ffi::lv_event_code_t_LV_EVENT_CLICKED => Self::Clicked,
            ffi::lv_event_code_t_LV_EVENT_RELEASED => Self::Released,
            ffi::lv_event_code_t_LV_EVENT_SCROLL_BEGIN => Self::ScrollBegin,
            ffi::lv_event_code_t_LV_EVENT_SCROLL_THROW_BEGIN => Self::ScrollThrowBegin,
            ffi::lv_event_code_t_LV_EVENT_SCROLL_END => Self::ScrollEnd,
            ffi::lv_event_code_t_LV_EVENT_SCROLL => Self::Scroll,
            ffi::lv_event_code_t_LV_EVENT_GESTURE => Self::Gesture,
            ffi::lv_event_code_t_LV_EVENT_KEY => Self::Key,
            ffi::lv_event_code_t_LV_EVENT_ROTARY => Self::Rotary,
            ffi::lv_event_code_t_LV_EVENT_FOCUSED => Self::Focused,
            ffi::lv_event_code_t_LV_EVENT_DEFOCUSED => Self::Defocused,
            ffi::lv_event_code_t_LV_EVENT_LEAVE => Self::Leave,
            ffi::lv_event_code_t_LV_EVENT_HIT_TEST => Self::HitTest,
            ffi::lv_event_code_t_LV_EVENT_INDEV_RESET => Self::InputDeviceReset,
            ffi::lv_event_code_t_LV_EVENT_HOVER_OVER => Self::HoverOver,
            ffi::lv_event_code_t_LV_EVENT_HOVER_LEAVE => Self::HoverLeave,
            ffi::lv_event_code_t_LV_EVENT_COVER_CHECK => Self::CoverCheck,
            ffi::lv_event_code_t_LV_EVENT_REFR_EXT_DRAW_SIZE => Self::RefreshExtDrawSize,
            ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN_BEGIN => Self::DrawMainBegin,
            ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN => Self::DrawMain,
            ffi::lv_event_code_t_LV_EVENT_DRAW_MAIN_END => Self::DrawMainEnd,
            ffi::lv_event_code_t_LV_EVENT_DRAW_POST_BEGIN => Self::DrawPostBegin,
            ffi::lv_event_code_t_LV_EVENT_DRAW_POST => Self::DrawPost,
            ffi::lv_event_code_t_LV_EVENT_DRAW_POST_END => Self::DrawPostEnd,
            ffi::lv_event_code_t_LV_EVENT_DRAW_TASK_ADDED => Self::DrawTaskAdded,
            ffi::lv_event_code_t_LV_EVENT_VALUE_CHANGED => Self::ValueChanged,
            ffi::lv_event_code_t_LV_EVENT_INSERT => Self::Insert,
            ffi::lv_event_code_t_LV_EVENT_REFRESH => Self::Refresh,
            ffi::lv_event_code_t_LV_EVENT_READY => Self::Ready,
            ffi::lv_event_code_t_LV_EVENT_CANCEL => Self::Cancel,
            ffi::lv_event_code_t_LV_EVENT_CREATE => Self::Create,
            ffi::lv_event_code_t_LV_EVENT_DELETE => Self::Delete,
            ffi::lv_event_code_t_LV_EVENT_CHILD_CHANGED => Self::ChildChanged,
            ffi::lv_event_code_t_LV_EVENT_CHILD_CREATED => Self::ChildCreated,
            ffi::lv_event_code_t_LV_EVENT_CHILD_DELETED => Self::ChildDeleted,
            ffi::lv_event_code_t_LV_EVENT_SCREEN_UNLOAD_START => Self::ScreenUnloadStart,
            ffi::lv_event_code_t_LV_EVENT_SCREEN_LOAD_START => Self::ScreenLoadStart,
            ffi::lv_event_code_t_LV_EVENT_SCREEN_LOADED => Self::ScreenLoaded,
            ffi::lv_event_code_t_LV_EVENT_SCREEN_UNLOADED => Self::ScreenUnloaded,
            ffi::lv_event_code_t_LV_EVENT_SIZE_CHANGED => Self::SizeChanged,
            ffi::lv_event_code_t_LV_EVENT_STYLE_CHANGED => Self::StyleChanged,
            ffi::lv_event_code_t_LV_EVENT_LAYOUT_CHANGED => Self::LayoutChanged,
            ffi::lv_event_code_t_LV_EVENT_GET_SELF_SIZE => Self::GetSelfSize,
            ffi::lv_event_code_t_LV_EVENT_INVALIDATE_AREA => Self::InvalidateArea,
            ffi::lv_event_code_t_LV_EVENT_RESOLUTION_CHANGED => Self::ResolutionChanged,
            ffi::lv_event_code_t_LV_EVENT_COLOR_FORMAT_CHANGED => Self::ColorFormatChanged,
            ffi::lv_event_code_t_LV_EVENT_REFR_REQUEST => Self::RefreshRequest,
            ffi::lv_event_code_t_LV_EVENT_REFR_START => Self::RefreshStart,
            ffi::lv_event_code_t_LV_EVENT_REFR_READY => Self::RefreshReady,
            ffi::lv_event_code_t_LV_EVENT_RENDER_START => Self::RenderStart,
            ffi::lv_event_code_t_LV_EVENT_RENDER_READY => Self::RenderReady,
            ffi::lv_event_code_t_LV_EVENT_FLUSH_START => Self::FlushStart,
            ffi::lv_event_code_t_LV_EVENT_FLUSH_FINISH => Self::FlushFinish,
            ffi::lv_event_code_t_LV_EVENT_FLUSH_WAIT_START => Self::FlushWaitStart,
            ffi::lv_event_code_t_LV_EVENT_FLUSH_WAIT_FINISH => Self::FlushWaitFinish,
            ffi::lv_event_code_t_LV_EVENT_VSYNC => Self::VerticalSynchronization,
            ffi::lv_event_code_t_LV_EVENT_LAST => Self::Last,
            ffi::lv_event_code_t_LV_EVENT_PREPROCESS => Self::Preprocess,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 1 => Self::Custom1,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 2 => Self::Custom2,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 3 => Self::CloseRequested,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 4 => Self::Custom4,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 5 => Self::Custom5,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 6 => Self::Custom6,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 7 => Self::Custom7,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 8 => Self::Custom8,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 9 => Self::Custom9,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 10 => Self::Custom10,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 11 => Self::Custom11,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 12 => Self::Custom12,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 13 => Self::Custom13,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 14 => Self::Custom14,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 15 => Self::Custom15,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 16 => Self::Custom16,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 17 => Self::Custom17,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 18 => Self::Custom18,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 19 => Self::Custom19,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 20 => Self::Custom20,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 21 => Self::Custom21,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 22 => Self::Custom22,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 23 => Self::Custom23,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 24 => Self::Custom24,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 25 => Self::Custom25,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 26 => Self::Custom26,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 27 => Self::Custom27,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 28 => Self::Custom28,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 29 => Self::Custom29,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 30 => Self::Custom30,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 31 => Self::Custom31,
            code if code == ffi::lv_event_code_t_LV_EVENT_LAST + 32 => Self::Custom32,
            _ => Self::Last,
        }
    }
}

impl From<EventKind> for ffi::lv_event_code_t {
    fn from(code: EventKind) -> Self {
        code.into_lvgl_code()
    }
}

impl From<ffi::lv_event_code_t> for EventKind {
    fn from(code: ffi::lv_event_code_t) -> Self {
        EventKind::from_lvgl_code(code)
    }
}
