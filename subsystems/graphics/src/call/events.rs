//! Translate native event semantics to the Xila event contract.
use super::event_code::{EVENT_CLOSE_REQUESTED, EventCode};
use crate::{EventKind, lvgl};

pub(super) fn event_code(event: EventKind) -> u32 {
    match event {
        EventKind::All => EventCode::All as u32,
        EventKind::Pressed => EventCode::Pressed as u32,
        EventKind::Pressing => EventCode::Pressing as u32,
        EventKind::PressLost => EventCode::PressLost as u32,
        EventKind::ShortClicked => EventCode::ShortClicked as u32,
        EventKind::SingleClicked => EventCode::SingleClicked as u32,
        EventKind::DoubleClicked => EventCode::DoubleClicked as u32,
        EventKind::TripleClicked => EventCode::TripleClicked as u32,
        EventKind::LongPressed => EventCode::LongPressed as u32,
        EventKind::LongPressedRepeat => EventCode::LongPressedRepeat as u32,
        EventKind::Clicked => EventCode::Clicked as u32,
        EventKind::Released => EventCode::Released as u32,
        EventKind::ScrollBegin => EventCode::ScrollBegin as u32,
        EventKind::ScrollThrowBegin => EventCode::ScrollThrowBegin as u32,
        EventKind::ScrollEnd => EventCode::ScrollEnd as u32,
        EventKind::Scroll => EventCode::Scroll as u32,
        EventKind::Gesture => EventCode::Gesture as u32,
        EventKind::Key => EventCode::Key as u32,
        EventKind::Rotary => EventCode::Rotary as u32,
        EventKind::Focused => EventCode::Focused as u32,
        EventKind::Defocused => EventCode::Defocused as u32,
        EventKind::Leave => EventCode::Leave as u32,
        EventKind::HitTest => EventCode::HitTest as u32,
        EventKind::InputDeviceReset => EventCode::IndevReset as u32,
        EventKind::HoverOver => EventCode::HoverOver as u32,
        EventKind::HoverLeave => EventCode::HoverLeave as u32,
        EventKind::CoverCheck => EventCode::CoverCheck as u32,
        EventKind::RefreshExtDrawSize => EventCode::RefreshExtDrawSize as u32,
        EventKind::DrawMainBegin => EventCode::DrawMainBegin as u32,
        EventKind::DrawMain => EventCode::DrawMain as u32,
        EventKind::DrawMainEnd => EventCode::DrawMainEnd as u32,
        EventKind::DrawPostBegin => EventCode::DrawPostBegin as u32,
        EventKind::DrawPost => EventCode::DrawPost as u32,
        EventKind::DrawPostEnd => EventCode::DrawPostEnd as u32,
        EventKind::DrawTaskAdded => EventCode::DrawTaskAdded as u32,
        EventKind::ValueChanged => EventCode::ValueChanged as u32,
        EventKind::Insert => EventCode::Insert as u32,
        EventKind::Refresh => EventCode::Refresh as u32,
        EventKind::Ready => EventCode::Ready as u32,
        EventKind::Cancel => EventCode::Cancel as u32,
        EventKind::Create => EventCode::Create as u32,
        EventKind::Delete => EventCode::Delete as u32,
        EventKind::ChildChanged => EventCode::ChildChanged as u32,
        EventKind::ChildCreated => EventCode::ChildCreated as u32,
        EventKind::ChildDeleted => EventCode::ChildDeleted as u32,
        EventKind::ScreenUnloadStart => EventCode::ScreenUnloadStart as u32,
        EventKind::ScreenLoadStart => EventCode::ScreenLoadStart as u32,
        EventKind::ScreenLoaded => EventCode::ScreenLoaded as u32,
        EventKind::ScreenUnloaded => EventCode::ScreenUnloaded as u32,
        EventKind::SizeChanged => EventCode::SizeChanged as u32,
        EventKind::StyleChanged => EventCode::StyleChanged as u32,
        EventKind::LayoutChanged => EventCode::LayoutChanged as u32,
        EventKind::GetSelfSize => EventCode::GetSelfSize as u32,
        EventKind::InvalidateArea => EventCode::InvalidateArea as u32,
        EventKind::ResolutionChanged => EventCode::ResolutionChanged as u32,
        EventKind::ColorFormatChanged => EventCode::ColorFormatChanged as u32,
        EventKind::RefreshRequest => EventCode::RefreshRequest as u32,
        EventKind::RefreshStart => EventCode::RefreshStart as u32,
        EventKind::RefreshReady => EventCode::RefreshReady as u32,
        EventKind::RenderStart => EventCode::RenderStart as u32,
        EventKind::RenderReady => EventCode::RenderReady as u32,
        EventKind::FlushStart => EventCode::FlushStart as u32,
        EventKind::FlushFinish => EventCode::FlushFinish as u32,
        EventKind::FlushWaitStart => EventCode::FlushWaitStart as u32,
        EventKind::FlushWaitFinish => EventCode::FlushWaitFinish as u32,
        EventKind::VerticalSynchronization => EventCode::Vsync as u32,
        EventKind::CloseRequested => EVENT_CLOSE_REQUESTED,
        EventKind::Preprocess => EventCode::Preprocess as u32,
        // Custom Xila events keep their offset from the contract's last event.
        _ => EventCode::Last as u32 + (event as u32 - lvgl::lv_event_code_t_LV_EVENT_LAST),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_events_use_the_xila_event_contract() {
        assert_eq!(EventCode::ValueChanged as u32, 35);
        assert_eq!(EventCode::Delete as u32, 41);
        assert_eq!(EventCode::Last as u32, 67);
        assert_eq!(EVENT_CLOSE_REQUESTED, 70);
        assert_eq!(
            event_code(EventKind::ValueChanged),
            EventCode::ValueChanged as u32
        );
        assert_eq!(event_code(EventKind::Delete), EventCode::Delete as u32);
        assert_eq!(event_code(EventKind::CloseRequested), EVENT_CLOSE_REQUESTED);
        assert_eq!(event_code(EventKind::Custom32), EventCode::Last as u32 + 32);
        assert_eq!(
            event_code(EventKind::Preprocess),
            EventCode::Preprocess as u32
        );
    }
}
