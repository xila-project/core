//! Fixed-capacity event ring owned by each window.
//!
//! The ring lives inline in the window object, so it never allocates, never
//! grows and never fragments a heap, however many events a burst produces.
//! Overflow is resolved by the policy documented on [`EventQueue`].

use crate::{Event, EventKind, lvgl};
use core::ptr;

/// Number of events a window can hold before its overflow policy applies.
pub const WINDOW_EVENT_QUEUE_CAPACITY: usize = 16;

/// Slots kept for the rest of each accepted press. A short press ends with
/// `Released`, `ShortClicked`, one of `SingleClicked`, `DoubleClicked` or
/// `TripleClicked`, and `Clicked`; a long press sends `LongPressed`,
/// `Released` and `Clicked`; a lost press ends with `PressLost`.
const PRESS_END_RESERVATION: u8 = 4;

/// Input devices whose current press, accepted or rejected, the queue
/// follows at once.
const PRESS_TRACKING_CAPACITY: usize = 4;

/// Minimum time between two loss reports, in LVGL ticks (milliseconds).
pub const LOSS_REPORT_INTERVAL: u32 = 1000;

// Indices are stored as `u8`.
const _: () =
    assert!(WINDOW_EVENT_QUEUE_CAPACITY > 0 && WINDOW_EVENT_QUEUE_CAPACITY <= u8::MAX as usize);
// A pointer press and a keypad press fit with their whole ends, and room
// remains for other events.
const _: () = assert!(2 * (PRESS_END_RESERVATION as usize + 1) < WINDOW_EVENT_QUEUE_CAPACITY);

/// The LVGL input device that sent an event, compared by address only. Null
/// for events sent by code.
pub type InputDevice = *const lvgl::lv_indev_t;

/// What [`EventQueue::push`] did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// The event was appended.
    Queued,
    /// The event replaced the queued event of the same kind and target that
    /// it supersedes, and moved to the back of the queue.
    Coalesced,
    /// The event was appended after evicting the oldest evictable event.
    EvictedOldest,
    /// The event was rejected.
    Dropped,
}

impl PushOutcome {
    /// Whether an event was lost: evicted or rejected.
    pub const fn lost_an_event(self) -> bool {
        matches!(self, Self::EvictedOldest | Self::Dropped)
    }
}

/// Events lost since the previous report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossReport {
    /// Events lost since the previous report.
    pub lost: u32,
    /// Events lost since the queue was created.
    pub total: u32,
}

/// How much an event may be sacrificed when the queue is full, from the
/// least to the most important. An arrival only evicts events of its own tier
/// or a less important one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// Notifications no consumer depends on: evicted first.
    Low,
    /// Pointer moves and press repeats: coalescible, and soon superseded by
    /// the next move or by the end of the press.
    Transient,
    /// State changes: coalescible, and the latest one per target must reach
    /// the consumer, which reads the new state when it handles it.
    State,
    /// Input, focus and window lifecycle events: never evicted, never reordered.
    Lifecycle,
}

impl Tier {
    /// Whether a newer event replaces the queued one of the same kind and
    /// target ("latest wins").
    const fn coalesces(self) -> bool {
        matches!(self, Self::Transient | Self::State)
    }
}

const fn tier(kind: EventKind) -> Tier {
    match kind {
        EventKind::Scroll
        | EventKind::ScrollBegin
        | EventKind::ScrollThrowBegin
        | EventKind::ScrollEnd
        | EventKind::LayoutChanged
        | EventKind::GetSelfSize
        | EventKind::HoverOver
        | EventKind::HoverLeave
        | EventKind::HitTest
        | EventKind::CoverCheck
        | EventKind::RefreshExtDrawSize
        | EventKind::DrawMainBegin
        | EventKind::DrawMain
        | EventKind::DrawMainEnd
        | EventKind::DrawPostBegin
        | EventKind::DrawPost
        | EventKind::DrawPostEnd
        | EventKind::DrawTaskAdded
        | EventKind::InvalidateArea
        | EventKind::Refresh
        | EventKind::Create => Tier::Low,
        EventKind::Pressing | EventKind::LongPressedRepeat => Tier::Transient,
        EventKind::ValueChanged | EventKind::SizeChanged | EventKind::StyleChanged => Tier::State,
        _ => Tier::Lifecycle,
    }
}

/// Whether the window must tell which input device sent an event of `kind`:
/// it takes part in a press, or it reports that a device was reset.
pub const fn carries_input_device(kind: EventKind) -> bool {
    press_role(kind).is_some() || matches!(kind, EventKind::InputDeviceReset)
}

/// The part an event plays in a press, from `Pressed` to its last click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressRole {
    Start,
    /// Movement and repeats: coalescible, so they need no reserved slot.
    Progress,
    LongPress,
    Release,
    Lost,
    Click,
    Final,
}

/// Whether `kind` starts or ends a press, or marks one of its steps: every
/// press event except the coalescible moves and repeats.
const fn is_press_boundary(kind: EventKind) -> bool {
    !matches!(press_role(kind), None | Some(PressRole::Progress))
}

const fn press_role(kind: EventKind) -> Option<PressRole> {
    match kind {
        EventKind::Pressed => Some(PressRole::Start),
        EventKind::Pressing | EventKind::LongPressedRepeat => Some(PressRole::Progress),
        EventKind::LongPressed => Some(PressRole::LongPress),
        EventKind::Released => Some(PressRole::Release),
        EventKind::PressLost => Some(PressRole::Lost),
        EventKind::ShortClicked
        | EventKind::SingleClicked
        | EventKind::DoubleClicked
        | EventKind::TripleClicked => Some(PressRole::Click),
        EventKind::Clicked => Some(PressRole::Final),
        _ => None,
    }
}

/// The current press of an input device, followed from its `Pressed` event
/// to its end.
#[derive(Debug, Clone, Copy)]
struct PressGroup {
    device: InputDevice,
    /// The pressed widget. A keypad may end the press on another one: the
    /// widget focused when the key is released.
    target: *mut lvgl::lv_obj_t,
    /// Whether `Pressed` was queued. The rest of a rejected press is dropped.
    accepted: bool,
    /// Whether `Released` arrived: only clicks can follow.
    released: bool,
    /// Slots still reserved for the end of an accepted press.
    reserved: u8,
}

impl PressGroup {
    const EMPTY: Self = Self {
        device: ptr::null(),
        target: ptr::null_mut(),
        accepted: false,
        released: false,
        reserved: 0,
    };
}

/// Bounded queue of window events in arrival order.
///
/// Events fall in four tiers, from the least to the most important:
///
/// - low: the scroll family, layout changes, hover, hit tests and drawing
///   notifications;
/// - transient: `Pressing` and `LongPressedRepeat`;
/// - state: `ValueChanged`, `SizeChanged` and `StyleChanged`;
/// - lifecycle: everything else, such as presses, releases, clicks, keys,
///   focus changes, `Ready`, `Cancel`, `Delete` and `CloseRequested`.
///
/// Transient and state events are "latest wins": a newer one replaces the
/// queued event of the same kind and target and moves to the back, unless a
/// lifecycle event for that target lies between them. The input device does
/// not take part in the match: the moves and value changes that two devices
/// make on one widget merge into the latest one. Consumers read the widget's
/// current state, and events carry no device, which would cost a word per
/// slot.
///
/// When the queue is full, an arrival evicts the oldest low event, or else the
/// oldest transient event, or else the oldest state event, but never an event
/// more important than itself. Lifecycle events are never evicted or
/// reordered. An arrival that finds nothing to evict is rejected.
///
/// Presses are followed per input device (press events sent by code, without
/// one, are queued by tier only). A `Pressed` event is accepted only
/// with room for the rest of its press (`LongPressed`, `Released` or
/// `PressLost`, then the clicks), which stays reserved until those events
/// arrive, the device presses again, is reset, or the window learns that it
/// was released or deleted ([`EventQueue::forget_ended_presses`]). Other
/// lifecycle events cannot take that room. When a `Pressed` event is
/// rejected, the rest of that press is dropped as well: the queue never drops
/// a `Pressed` event while keeping the end of its press.
///
/// Every evicted or rejected event is counted, and
/// [`EventQueue::take_loss_report`] summarizes the losses at most once per
/// [`LOSS_REPORT_INTERVAL`].
pub struct EventQueue {
    slots: [Option<Event>; WINDOW_EVENT_QUEUE_CAPACITY],
    head: u8,
    length: u8,
    /// Number of valid entries at the start of `presses`, oldest first.
    press_count: u8,
    presses: [PressGroup; PRESS_TRACKING_CAPACITY],
    dropped: u32,
    unreported: u32,
    last_report: Option<u32>,
}

impl EventQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; WINDOW_EVENT_QUEUE_CAPACITY],
            head: 0,
            length: 0,
            press_count: 0,
            presses: [PressGroup::EMPTY; PRESS_TRACKING_CAPACITY],
            dropped: 0,
            unreported: 0,
            last_report: None,
        }
    }

    pub const fn len(&self) -> usize {
        self.length as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub const fn is_full(&self) -> bool {
        self.length as usize == WINDOW_EVENT_QUEUE_CAPACITY
    }

    /// Whether a press is followed: its target must be forgotten when it dies.
    pub const fn follows_presses(&self) -> bool {
        self.press_count > 0
    }

    /// Number of events lost to overflow since creation, including rejections
    /// and evictions.
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Whether losses happened since the last report.
    pub const fn has_unreported_losses(&self) -> bool {
        self.unreported > 0
    }

    /// Summarize the losses since the last report, unless there were none or
    /// the last report is less than [`LOSS_REPORT_INTERVAL`] ticks old. `now`
    /// is the current LVGL tick.
    pub fn take_loss_report(&mut self, now: u32) -> Option<LossReport> {
        if self.unreported == 0 {
            return None;
        }
        if let Some(last) = self.last_report
            && now.wrapping_sub(last) < LOSS_REPORT_INTERVAL
        {
            return None;
        }
        self.last_report = Some(now);
        Some(LossReport {
            lost: core::mem::take(&mut self.unreported),
            total: self.dropped,
        })
    }

    /// Append an event that `device` sent, according to the queue's overflow
    /// policy. `device` only matters for press events and device resets (see
    /// [`carries_input_device`]). Press events sent by code, without a device,
    /// are queued by tier only: nothing ends such a press reliably.
    pub fn push(&mut self, event: Event, device: InputDevice) -> PushOutcome {
        if event.code == EventKind::InputDeviceReset && !device.is_null() {
            // LVGL forgot the widget the device pressed: it never ends that
            // press, and a new press starts if the device is still held.
            self.forget_presses(|group| group.device == device);
        }
        let Some(role) = press_role(event.code).filter(|_| !device.is_null()) else {
            return self.insert(event, false);
        };
        if role == PressRole::Start {
            return self.push_press(event, device);
        }
        // LVGL sends the rest of a press from the same device, possibly to
        // another widget: a keypad releases on the focused widget.
        let Some(index) = self.press_index(device) else {
            return self.insert(event, false);
        };

        let group = &mut self.presses[index];
        let accepted = group.accepted;
        let uses_reservation = accepted && role != PressRole::Progress && group.reserved > 0;
        if uses_reservation {
            group.reserved -= 1;
        }
        match role {
            PressRole::Release => group.released = true,
            PressRole::Lost | PressRole::Final => self.forget_press(index),
            _ => {}
        }

        if accepted {
            self.insert(event, uses_reservation)
        } else {
            self.lose()
        }
    }

    fn push_press(&mut self, event: Event, device: InputDevice) -> PushOutcome {
        // A device presses one widget at a time, so a new press ends its
        // previous one, even one LVGL never ended (an encoder long press that
        // leaves edit mode, or a widget disabled while pressed). LVGL also
        // sends a release and its clicks back to back, so a new press means
        // every released press has ended.
        self.forget_presses(|group| group.released || group.device == device);

        let accepted =
            self.lifecycle_count() + 1 + self.reserved() + PRESS_END_RESERVATION as usize
                <= WINDOW_EVENT_QUEUE_CAPACITY;
        self.follow_press(PressGroup {
            device,
            target: event.target,
            accepted,
            released: false,
            reserved: if accepted { PRESS_END_RESERVATION } else { 0 },
        });
        if accepted {
            self.insert(event, false)
        } else {
            self.lose()
        }
    }

    /// Queue `event` by its tier. `uses_reservation` means a slot reserved
    /// for the end of its press was just released for it.
    fn insert(&mut self, event: Event, uses_reservation: bool) -> PushOutcome {
        let tier = tier(event.code);
        if tier.coalesces()
            && let Some(index) = self.superseded_by(&event)
        {
            self.remove(index);
            self.append(event);
            return PushOutcome::Coalesced;
        }
        // Lifecycle events never take the room reserved for press ends.
        if tier == Tier::Lifecycle
            && !uses_reservation
            && self.lifecycle_count() + self.reserved() >= WINDOW_EVENT_QUEUE_CAPACITY
        {
            return self.lose();
        }
        if !self.is_full() {
            self.append(event);
            return PushOutcome::Queued;
        }

        let victim = [Tier::Low, Tier::Transient, Tier::State]
            .into_iter()
            .take_while(|candidate| *candidate <= tier)
            .find_map(|candidate| self.oldest(candidate));
        match victim {
            Some(index) => {
                self.remove(index);
                self.append(event);
                self.lose();
                PushOutcome::EvictedOldest
            }
            None => self.lose(),
        }
    }

    pub fn front(&self) -> Option<&Event> {
        self.get(0)
    }

    pub fn pop_front(&mut self) -> Option<Event> {
        if self.is_empty() {
            return None;
        }
        let event = self.slots[self.head as usize].take();
        self.head = ((self.head as usize + 1) % WINDOW_EVENT_QUEUE_CAPACITY) as u8;
        self.length -= 1;
        event
    }

    /// Forget the press of every input device that `may_continue` rejects:
    /// LVGL will send nothing more for that press. The window rejects a device
    /// that is released or deleted, unless LVGL is reading it right now.
    /// Forgetting an accepted press gives its reservation back, and
    /// forgetting a rejected one is safe because its end was already sent.
    /// `may_continue` must only compare the address: the device may already
    /// be deleted.
    pub fn forget_ended_presses(&mut self, mut may_continue: impl FnMut(InputDevice) -> bool) {
        self.forget_presses(|group| !may_continue(group.device));
    }

    /// Remove every event aimed at `target`, preserving the order of the rest,
    /// and give up the reservation of an accepted press of `target`. Press
    /// events other than moves and repeats are kept with a null target
    /// instead: removing a `Pressed` event would let the end of its press
    /// reach the consumer alone, since a keypad ends its press on another
    /// widget. Only compares addresses; never dereferences `target`.
    pub fn discard_target(&mut self, target: *mut lvgl::lv_obj_t) {
        let mut kept = 0;
        for index in 0..self.len() {
            let source = self.physical(index);
            let Some(mut event) = self.slots[source].take() else {
                continue;
            };
            let discarded = event.target == target;
            if discarded {
                event.target = ptr::null_mut();
            }
            if !discarded || is_press_boundary(event.code) {
                let destination = self.physical(kept);
                self.slots[destination] = Some(event);
                kept += 1;
            }
        }
        self.length = kept as u8;
        self.forget_accepted_presses(|pressed| pressed == target);
    }

    /// Keep every event but null the ones aimed at `target`, and give up the
    /// reservation of an accepted press of `target`.
    /// Only compares addresses; never dereferences `target`.
    pub fn clear_target(&mut self, target: *mut lvgl::lv_obj_t) {
        for event in self.slots.iter_mut().flatten() {
            if event.target == target {
                event.target = ptr::null_mut();
            }
        }
        self.forget_accepted_presses(|pressed| pressed == target);
    }

    /// Null the target of every event whose target `is_live` rejects, keep
    /// the events, and give up the reservations of accepted presses of
    /// rejected targets. `is_live` is called once per distinct run of targets
    /// and once per accepted press, and must only compare the address; the
    /// targets may already be deleted.
    pub fn clear_dead_targets(&mut self, mut is_live: impl FnMut(*mut lvgl::lv_obj_t) -> bool) {
        let mut last = (ptr::null_mut(), true);
        for event in self.slots.iter_mut().flatten() {
            if event.target.is_null() {
                continue;
            }
            if last.0 != event.target {
                last = (event.target, is_live(event.target));
            }
            if !last.1 {
                event.target = ptr::null_mut();
            }
        }
        self.forget_accepted_presses(|pressed| !is_live(pressed));
    }

    /// Forget the accepted presses whose widget `is_gone`. Forgetting one
    /// only gives up its reservation. A rejected press is kept: its device
    /// may still end it on another widget, and that end must be dropped.
    fn forget_accepted_presses(&mut self, mut is_gone: impl FnMut(*mut lvgl::lv_obj_t) -> bool) {
        self.forget_presses(|group| group.accepted && is_gone(group.target));
    }

    fn lose(&mut self) -> PushOutcome {
        self.dropped = self.dropped.saturating_add(1);
        self.unreported = self.unreported.saturating_add(1);
        PushOutcome::Dropped
    }

    /// The queued event that `event` supersedes: the newest one of the same
    /// kind and target, unless a lifecycle event for that target follows it.
    fn superseded_by(&self, event: &Event) -> Option<usize> {
        for index in (0..self.len()).rev() {
            let queued = self.get(index)?;
            if queued.target != event.target {
                continue;
            }
            if queued.code == event.code {
                return Some(index);
            }
            if tier(queued.code) == Tier::Lifecycle {
                return None;
            }
        }
        None
    }

    fn oldest(&self, wanted: Tier) -> Option<usize> {
        (0..self.len()).find(|&index| {
            self.get(index)
                .is_some_and(|event| tier(event.code) == wanted)
        })
    }

    fn lifecycle_count(&self) -> usize {
        (0..self.len())
            .filter(|&index| {
                self.get(index)
                    .is_some_and(|event| tier(event.code) == Tier::Lifecycle)
            })
            .count()
    }

    fn reserved(&self) -> usize {
        self.followed_presses()
            .iter()
            .map(|group| group.reserved as usize)
            .sum()
    }

    fn followed_presses(&self) -> &[PressGroup] {
        &self.presses[..self.press_count as usize]
    }

    fn press_index(&self, device: InputDevice) -> Option<usize> {
        self.followed_presses()
            .iter()
            .position(|group| group.device == device)
    }

    /// Start following a press. With more than [`PRESS_TRACKING_CAPACITY`]
    /// input devices pressing at once, this makes room by forgetting the
    /// oldest accepted press, or else the oldest press; only the latter can
    /// let the end of a rejected press through.
    fn follow_press(&mut self, group: PressGroup) {
        if self.press_count as usize == PRESS_TRACKING_CAPACITY {
            let index = self
                .followed_presses()
                .iter()
                .position(|group| group.accepted)
                .unwrap_or(0);
            self.forget_press(index);
        }
        self.presses[self.press_count as usize] = group;
        self.press_count += 1;
    }

    fn forget_press(&mut self, index: usize) {
        let count = self.press_count as usize;
        self.presses.copy_within(index + 1..count, index);
        self.presses[count - 1] = PressGroup::EMPTY;
        self.press_count -= 1;
    }

    fn forget_presses(&mut self, mut forget: impl FnMut(&PressGroup) -> bool) {
        let mut index = 0;
        while index < self.press_count as usize {
            if forget(&self.presses[index]) {
                self.forget_press(index);
            } else {
                index += 1;
            }
        }
    }

    fn physical(&self, index: usize) -> usize {
        (self.head as usize + index) % WINDOW_EVENT_QUEUE_CAPACITY
    }

    fn get(&self, index: usize) -> Option<&Event> {
        if index < self.len() {
            self.slots[self.physical(index)].as_ref()
        } else {
            None
        }
    }

    /// Append to a queue that is not full.
    fn append(&mut self, event: Event) {
        let slot = self.physical(self.len());
        self.slots[slot] = Some(event);
        self.length += 1;
    }

    /// Remove the event at logical `index`, shifting later events forward.
    fn remove(&mut self, index: usize) {
        if index == 0 {
            self.pop_front();
            return;
        }
        for position in index..self.len() - 1 {
            let (from, to) = (self.physical(position + 1), self.physical(position));
            self.slots[to] = self.slots[from].take();
        }
        let last = self.physical(self.len() - 1);
        self.slots[last] = None;
        self.length -= 1;
    }
}

impl Default for EventQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;
    use alloc::vec::Vec;

    impl EventQueue {
        /// Push an event from the pointer.
        fn send(&mut self, event: Event) -> PushOutcome {
            self.push(event, device(1))
        }

        /// Push an event sent by code, without an input device.
        fn send_by_code(&mut self, event: Event) -> PushOutcome {
            self.push(event, ptr::null())
        }
    }

    fn device(address: usize) -> InputDevice {
        address as InputDevice
    }

    fn object(address: usize) -> *mut lvgl::lv_obj_t {
        address as *mut lvgl::lv_obj_t
    }

    fn event(code: EventKind, address: usize) -> Event {
        Event::new(code, object(address), None)
    }

    fn pointer_move(address: usize, x: i32) -> Event {
        let mut moved = event(EventKind::Pressing, address);
        moved.pointer = Some([x, 0, 100, 100]);
        moved
    }

    fn drain(queue: &mut EventQueue) -> Vec<(EventKind, usize)> {
        core::iter::from_fn(|| queue.pop_front())
            .map(|event| (event.code, event.target as usize))
            .collect()
    }

    fn only(drained: &[(EventKind, usize)], target: usize) -> Vec<EventKind> {
        drained
            .iter()
            .filter(|(_, address)| *address == target)
            .map(|(code, _)| *code)
            .collect()
    }

    /// Fill the queue with lifecycle events for `target` until one is rejected.
    fn flood(queue: &mut EventQueue, code: EventKind, target: usize) -> usize {
        let mut queued = 0;
        while queue.send(event(code, target)) != PushOutcome::Dropped {
            queued += 1;
        }
        queued
    }

    const SHORT_PRESS_END: [EventKind; 4] = [
        EventKind::Released,
        EventKind::ShortClicked,
        EventKind::SingleClicked,
        EventKind::Clicked,
    ];

    #[test]
    fn pops_in_push_order_across_wraparound() {
        let mut queue = EventQueue::new();
        for round in 0..5 {
            for index in 0..10 {
                assert_eq!(
                    queue.send(event(EventKind::Clicked, round * 100 + index + 1)),
                    PushOutcome::Queued
                );
            }
            let expected: Vec<_> = (0..10)
                .map(|index| (EventKind::Clicked, round * 100 + index + 1))
                .collect();
            assert_eq!(drain(&mut queue), expected);
            assert!(queue.is_empty());
        }
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn peek_front_does_not_consume() {
        let mut queue = EventQueue::new();
        assert!(queue.front().is_none());
        queue.send(event(EventKind::Pressed, 1));
        queue.send(event(EventKind::Released, 1));
        assert_eq!(queue.front().unwrap().code, EventKind::Pressed);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop_front().unwrap().code, EventKind::Pressed);
        assert_eq!(queue.front().unwrap().code, EventKind::Released);
    }

    #[test]
    fn pointer_moves_for_one_target_keep_the_latest() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        assert_eq!(queue.send(pointer_move(1, 10)), PushOutcome::Queued);
        for x in 11..200 {
            assert_eq!(queue.send(pointer_move(1, x)), PushOutcome::Coalesced);
        }
        assert_eq!(queue.len(), 2);
        queue.pop_front();
        assert_eq!(queue.pop_front().unwrap().pointer, Some([199, 0, 100, 100]));
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn coalescing_moves_the_latest_event_to_the_back_even_when_not_at_the_tail() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        queue.send(pointer_move(1, 1));
        queue.send(event(EventKind::ValueChanged, 1));
        queue.send(event(EventKind::Scroll, 1));
        queue.send(pointer_move(2, 5));
        assert_eq!(queue.send(pointer_move(1, 2)), PushOutcome::Coalesced);
        assert_eq!(
            queue.send(event(EventKind::ValueChanged, 1)),
            PushOutcome::Coalesced
        );
        assert_eq!(
            queue.send(event(EventKind::LongPressedRepeat, 1)),
            PushOutcome::Queued
        );
        assert_eq!(
            queue.send(event(EventKind::LongPressedRepeat, 1)),
            PushOutcome::Coalesced
        );
        let mut positions = Vec::new();
        while let Some(event) = queue.pop_front() {
            positions.push((event.code, event.target as usize, event.pointer));
        }
        assert_eq!(
            positions,
            [
                (EventKind::Pressed, 1, None),
                (EventKind::Scroll, 1, None),
                (EventKind::Pressing, 2, Some([5, 0, 100, 100])),
                (EventKind::Pressing, 1, Some([2, 0, 100, 100])),
                (EventKind::ValueChanged, 1, None),
                (EventKind::LongPressedRepeat, 1, None),
            ]
        );
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn coalescing_never_crosses_a_lifecycle_event_of_the_same_target() {
        let mut queue = EventQueue::new();
        queue.send(pointer_move(1, 1));
        queue.send(event(EventKind::Key, 1));
        assert_eq!(queue.send(pointer_move(1, 2)), PushOutcome::Queued);
        queue.send(event(EventKind::ValueChanged, 2));
        queue.send(event(EventKind::Clicked, 3));
        assert_eq!(
            queue.send(event(EventKind::ValueChanged, 2)),
            PushOutcome::Coalesced
        );
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Pressing, 1),
                (EventKind::Key, 1),
                (EventKind::Pressing, 1),
                (EventKind::Clicked, 3),
                (EventKind::ValueChanged, 2),
            ]
        );
    }

    #[test]
    fn full_queue_never_evicts_a_more_important_event() {
        let mut queue = EventQueue::new();
        for address in 1..=5 {
            queue.send(event(EventKind::ValueChanged, address));
            queue.send(pointer_move(address, 0));
            queue.send(event(EventKind::ScrollEnd, address));
        }
        queue.send(event(EventKind::Focused, 99));
        assert!(queue.is_full());
        let expect = |queue: &mut EventQueue, code, address, outcome| {
            assert_eq!(queue.send(event(code, address)), outcome, "{code:?}");
        };
        use PushOutcome::{Dropped, EvictedOldest};

        // Low and transient arrivals evict low events first.
        expect(&mut queue, EventKind::ScrollBegin, 50, EvictedOldest);
        expect(&mut queue, EventKind::Pressing, 60, EvictedOldest);
        for address in 10..=13 {
            expect(&mut queue, EventKind::Focused, address, EvictedOldest);
        }
        // No low event is left: a low arrival is rejected, the others evict
        // the oldest transient event.
        expect(&mut queue, EventKind::ScrollBegin, 51, Dropped);
        expect(&mut queue, EventKind::Pressing, 61, EvictedOldest);
        for address in 14..=19 {
            expect(&mut queue, EventKind::Focused, address, EvictedOldest);
        }
        // Only state and lifecycle events are left: a transient arrival is
        // rejected, a state or lifecycle arrival evicts the oldest state event.
        expect(&mut queue, EventKind::Pressing, 62, Dropped);
        expect(&mut queue, EventKind::SizeChanged, 70, EvictedOldest);
        expect(&mut queue, EventKind::Focused, 20, EvictedOldest);

        let mut expected: Vec<_> = (3..=5)
            .map(|address| (EventKind::ValueChanged, address))
            .collect();
        expected.push((EventKind::Focused, 99));
        expected.extend((10..=19).map(|address| (EventKind::Focused, address)));
        expected.push((EventKind::SizeChanged, 70));
        expected.push((EventKind::Focused, 20));
        assert_eq!(drain(&mut queue), expected);
        assert_eq!(queue.dropped(), 17);
    }

    /// The window is resized while a list inside it is flung, and the consumer
    /// is busy: the scroll burst must not push out the resize.
    #[test]
    fn a_scroll_burst_keeps_the_latest_resize() {
        let mut queue = EventQueue::new();
        let (window, list) = (1, 2);
        queue.send(event(EventKind::SizeChanged, window));
        queue.send(event(EventKind::StyleChanged, window));
        for step in 0..500 {
            queue.send(event(EventKind::Scroll, list));
            if step == 250 {
                assert_eq!(
                    queue.send(event(EventKind::SizeChanged, window)),
                    PushOutcome::Coalesced
                );
            }
            if step % 50 == 0 {
                queue.send(event(EventKind::ScrollBegin, list));
                queue.send(event(EventKind::ScrollEnd, list));
            }
        }
        let drained = drain(&mut queue);
        assert_eq!(
            only(&drained, window),
            [EventKind::StyleChanged, EventKind::SizeChanged]
        );
        assert_eq!(drained.len(), WINDOW_EVENT_QUEUE_CAPACITY);
    }

    #[test]
    fn lifecycle_events_are_never_evicted() {
        let mut queue = EventQueue::new();
        for address in 1..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.send(event(EventKind::Clicked, address));
        }
        for code in [
            EventKind::Clicked,
            EventKind::Pressing,
            EventKind::ValueChanged,
            EventKind::Scroll,
            EventKind::CloseRequested,
        ] {
            assert_eq!(queue.send(event(code, 99)), PushOutcome::Dropped);
        }
        assert_eq!(
            drain(&mut queue),
            (1..=WINDOW_EVENT_QUEUE_CAPACITY)
                .map(|address| (EventKind::Clicked, address))
                .collect::<Vec<_>>()
        );
        assert_eq!(queue.dropped(), 5);
    }

    #[test]
    fn events_are_sorted_in_tiers() {
        for code in [EventKind::Pressing, EventKind::LongPressedRepeat] {
            assert_eq!(tier(code), Tier::Transient, "{code:?}");
        }
        for code in [
            EventKind::ValueChanged,
            EventKind::SizeChanged,
            EventKind::StyleChanged,
        ] {
            assert_eq!(tier(code), Tier::State, "{code:?}");
        }
        for code in [
            EventKind::Scroll,
            EventKind::ScrollBegin,
            EventKind::ScrollThrowBegin,
            EventKind::ScrollEnd,
            EventKind::LayoutChanged,
            EventKind::HoverOver,
            EventKind::HoverLeave,
            EventKind::DrawMain,
            EventKind::Refresh,
            EventKind::InvalidateArea,
            EventKind::HitTest,
            EventKind::Create,
        ] {
            assert_eq!(tier(code), Tier::Low, "{code:?}");
        }
        for code in [
            EventKind::Pressed,
            EventKind::Released,
            EventKind::PressLost,
            EventKind::Clicked,
            EventKind::ShortClicked,
            EventKind::LongPressed,
            EventKind::Key,
            EventKind::Focused,
            EventKind::Defocused,
            EventKind::CloseRequested,
            EventKind::Delete,
            EventKind::Ready,
            EventKind::Cancel,
            EventKind::Custom1,
            EventKind::Custom2,
        ] {
            assert_eq!(tier(code), Tier::Lifecycle, "{code:?}");
        }
    }

    /// A slider drag inside one LVGL poll: `Pressing` and `ValueChanged`
    /// alternate hundreds of times while unrelated input fills the queue.
    #[test]
    fn slider_drag_burst_keeps_the_press_whole() {
        let mut queue = EventQueue::new();
        assert_eq!(
            queue.send(event(EventKind::Pressed, 1)),
            PushOutcome::Queued
        );
        for step in 0..512 {
            queue.send(pointer_move(1, step));
            queue.send(event(EventKind::ValueChanged, 1));
            queue.send(event(EventKind::Scroll, 3));
            queue.send_by_code(event(EventKind::Clicked, 2));
        }
        for code in SHORT_PRESS_END {
            assert_ne!(queue.send(event(code, 1)), PushOutcome::Dropped, "{code:?}");
        }
        assert!(queue.dropped() > 0);

        let drained = drain(&mut queue);
        assert_eq!(
            only(&drained, 1),
            [
                EventKind::Pressed,
                EventKind::Released,
                EventKind::ShortClicked,
                EventKind::SingleClicked,
                EventKind::Clicked,
            ],
            "drag moves are evicted for the unrelated clicks, the press is kept whole"
        );
        assert!(only(&drained, 2).len() >= WINDOW_EVENT_QUEUE_CAPACITY / 2);
    }

    /// A held button: `LongPressed`, then `Pressing` and `LongPressedRepeat`
    /// alternate while the user types into another widget.
    #[test]
    fn held_button_while_typing_keeps_the_press_whole() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        queue.send(event(EventKind::Focused, 2));
        for step in 0..300 {
            if step == 3 {
                assert_eq!(
                    queue.send(event(EventKind::LongPressed, 1)),
                    PushOutcome::Queued
                );
            }
            queue.send(pointer_move(1, step));
            queue.send(event(EventKind::LongPressedRepeat, 1));
            queue.send(Event::new(
                EventKind::Key,
                object(2),
                Some(Key::Character(b'a')),
            ));
            queue.send(event(EventKind::ValueChanged, 2));
        }
        for code in [EventKind::Released, EventKind::Clicked] {
            assert_ne!(queue.send(event(code, 1)), PushOutcome::Dropped, "{code:?}");
        }
        assert!(queue.dropped() > 0, "keys beyond the free room are lost");

        let drained = drain(&mut queue);
        assert_eq!(
            only(&drained, 1)
                .into_iter()
                .filter(|code| tier(*code) == Tier::Lifecycle)
                .collect::<Vec<_>>(),
            [
                EventKind::Pressed,
                EventKind::LongPressed,
                EventKind::Released,
                EventKind::Clicked,
            ]
        );
        let first_key = drained
            .iter()
            .position(|(code, _)| *code == EventKind::Key)
            .unwrap();
        assert_eq!(drained[1], (EventKind::Focused, 2));
        assert!(first_key > 1);
    }

    #[test]
    fn a_rejected_press_drops_the_rest_of_its_press() {
        let mut queue = EventQueue::new();
        let room = flood(&mut queue, EventKind::Key, 9);
        assert_eq!(room, WINDOW_EVENT_QUEUE_CAPACITY);
        queue.pop_front();
        queue.pop_front();
        // Two free slots: not enough for the press and its end.
        assert_eq!(
            queue.send(event(EventKind::Pressed, 1)),
            PushOutcome::Dropped
        );
        for code in [
            EventKind::Pressing,
            EventKind::LongPressed,
            EventKind::LongPressedRepeat,
            EventKind::Released,
            EventKind::ShortClicked,
            EventKind::DoubleClicked,
            EventKind::Clicked,
        ] {
            assert_eq!(queue.send(event(code, 1)), PushOutcome::Dropped, "{code:?}");
        }
        // A press lost to a slide onto another widget is dropped too.
        assert_eq!(
            queue.send(event(EventKind::Pressed, 2)),
            PushOutcome::Dropped
        );
        assert_eq!(
            queue.send(event(EventKind::PressLost, 2)),
            PushOutcome::Dropped
        );
        let drained = drain(&mut queue);
        assert!(only(&drained, 1).is_empty());
        assert!(only(&drained, 2).is_empty());
        assert_eq!(queue.dropped(), 1 + 1 + 7 + 2);

        // The next press of the same target is accepted again.
        assert_eq!(
            queue.send(event(EventKind::Pressed, 1)),
            PushOutcome::Queued
        );
        assert_eq!(
            queue.send(event(EventKind::Released, 1)),
            PushOutcome::Queued
        );
        assert_eq!(
            drain(&mut queue),
            [(EventKind::Pressed, 1), (EventKind::Released, 1)]
        );
    }

    #[test]
    fn an_accepted_press_reserves_room_for_its_end() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        let room = flood(&mut queue, EventKind::Key, 9);
        assert_eq!(
            room,
            WINDOW_EVENT_QUEUE_CAPACITY - 1 - PRESS_END_RESERVATION as usize
        );
        for code in SHORT_PRESS_END {
            assert_eq!(queue.send(event(code, 1)), PushOutcome::Queued, "{code:?}");
        }
        assert!(queue.is_full());
        assert!(!queue.follows_presses(), "the click ends the press");
        let drained = drain(&mut queue);
        assert_eq!(drained[0], (EventKind::Pressed, 1));
        assert_eq!(only(&drained, 1)[1..], SHORT_PRESS_END);
    }

    #[test]
    fn a_full_queue_of_moves_still_takes_the_end_of_the_press() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        for address in 2..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.send(pointer_move(address, 0));
        }
        assert!(queue.is_full());
        for code in SHORT_PRESS_END {
            assert_eq!(
                queue.send(event(code, 1)),
                PushOutcome::EvictedOldest,
                "{code:?}"
            );
        }
        let drained = drain(&mut queue);
        assert_eq!(drained[0], (EventKind::Pressed, 1));
        assert_eq!(only(&drained, 1)[1..], SHORT_PRESS_END);
    }

    #[test]
    fn press_lost_and_clicks_give_back_the_reservation() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        queue.send(event(EventKind::PressLost, 1));
        assert!(!queue.follows_presses());
        queue.send(event(EventKind::Pressed, 2));
        queue.send(event(EventKind::LongPressed, 2));
        queue.send(event(EventKind::Released, 2));
        queue.send(event(EventKind::Clicked, 2));
        assert!(!queue.follows_presses());
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 6
        );
    }

    #[test]
    fn a_release_without_click_keeps_its_reservation_until_the_next_press() {
        let mut queue = EventQueue::new();
        // A scroll ends the press with a release and no click.
        queue.send(event(EventKind::Pressed, 1));
        queue.send(event(EventKind::Released, 1));
        assert_eq!(drain(&mut queue).len(), 2);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 3
        );
        drain(&mut queue);
        queue.send(event(EventKind::Pressed, 2));
        queue.send(event(EventKind::PressLost, 2));
        assert!(!queue.follows_presses());
        drain(&mut queue);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY
        );
    }

    #[test]
    fn a_press_without_end_holds_one_reservation_until_its_device_presses_again() {
        let mut queue = EventQueue::new();
        let encoder = device(3);
        // An encoder long press that leaves edit mode sends nothing after
        // `Pressed`, twice.
        for target in [1, 2] {
            assert_eq!(
                queue.push(event(EventKind::Pressed, target), encoder),
                PushOutcome::Queued
            );
        }
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 2 - PRESS_END_RESERVATION as usize
        );
        drain(&mut queue);
        // The next press of the encoder ends and reserves for itself only.
        queue.push(event(EventKind::Pressed, 3), encoder);
        queue.push(event(EventKind::Released, 3), encoder);
        queue.push(event(EventKind::Clicked, 3), encoder);
        assert!(!queue.follows_presses());
        drain(&mut queue);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY
        );
    }

    #[test]
    fn a_pointer_press_stays_whole_while_the_encoder_presses_twice() {
        let mut queue = EventQueue::new();
        let (pointer, encoder) = (device(1), device(3));
        queue.push(event(EventKind::Pressed, 1), pointer);
        queue.push(event(EventKind::Pressed, 7), encoder);
        queue.push(pointer_move(1, 4), pointer);
        queue.push(event(EventKind::Pressed, 8), encoder);
        queue.push(event(EventKind::Released, 8), encoder);
        queue.push(event(EventKind::Clicked, 8), encoder);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 5 - PRESS_END_RESERVATION as usize,
            "only the pointer press still holds a reservation"
        );
        for code in SHORT_PRESS_END {
            assert_ne!(
                queue.push(event(code, 1), pointer),
                PushOutcome::Dropped,
                "{code:?}"
            );
        }
        let drained = drain(&mut queue);
        assert_eq!(
            only(&drained, 1)
                .into_iter()
                .filter(|code| tier(*code) == Tier::Lifecycle)
                .collect::<Vec<_>>(),
            [
                EventKind::Pressed,
                EventKind::Released,
                EventKind::ShortClicked,
                EventKind::SingleClicked,
                EventKind::Clicked,
            ]
        );
        assert_eq!(
            only(&drained, 8),
            [EventKind::Pressed, EventKind::Released, EventKind::Clicked]
        );
    }

    #[test]
    fn a_keypad_press_ends_on_the_focused_widget() {
        let mut queue = EventQueue::new();
        let keypad = device(2);
        queue.push(event(EventKind::Pressed, 1), keypad);
        flood(&mut queue, EventKind::Key, 9);
        // The focus moved to widget 2 before the key was released.
        for code in SHORT_PRESS_END {
            assert_eq!(
                queue.push(event(code, 2), keypad),
                PushOutcome::Queued,
                "{code:?}"
            );
        }
        let drained = drain(&mut queue);
        assert_eq!(only(&drained, 1), [EventKind::Pressed]);
        assert_eq!(only(&drained, 2), SHORT_PRESS_END);

        // The end of a rejected keypad press is dropped on any widget.
        flood(&mut queue, EventKind::Key, 9);
        assert_eq!(
            queue.push(event(EventKind::Pressed, 1), keypad),
            PushOutcome::Dropped
        );
        drain(&mut queue);
        for code in SHORT_PRESS_END {
            assert_eq!(
                queue.push(event(code, 2), keypad),
                PushOutcome::Dropped,
                "{code:?}"
            );
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn two_devices_pressing_one_widget_keep_separate_presses() {
        let mut queue = EventQueue::new();
        let (pointer, keypad) = (device(1), device(2));
        queue.push(event(EventKind::Pressed, 1), pointer);
        queue.push(event(EventKind::Pressed, 1), keypad);
        assert_eq!(queue.followed_presses().len(), 2);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 2 - 2 * PRESS_END_RESERVATION as usize
        );
        for code in SHORT_PRESS_END {
            assert_eq!(queue.push(event(code, 1), pointer), PushOutcome::Queued);
        }
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(queue.followed_presses()[0].device, keypad);
        for code in [
            EventKind::LongPressed,
            EventKind::Released,
            EventKind::Clicked,
        ] {
            assert_eq!(queue.push(event(code, 1), keypad), PushOutcome::Queued);
        }
        assert!(!queue.follows_presses());
    }

    #[test]
    fn presses_sent_by_code_are_queued_by_tier_only() {
        let mut queue = EventQueue::new();
        flood(&mut queue, EventKind::Key, 9);
        assert_eq!(
            queue.send_by_code(event(EventKind::Pressed, 1)),
            PushOutcome::Dropped
        );
        assert!(!queue.follows_presses());
        drain(&mut queue);
        // A rejected press sent by code never blocks later clicks sent by code.
        for code in [EventKind::Released, EventKind::Clicked] {
            assert_eq!(
                queue.send_by_code(event(code, 2)),
                PushOutcome::Queued,
                "{code:?}"
            );
        }
        // And a press sent by code reserves nothing.
        queue.send_by_code(event(EventKind::Pressed, 3));
        assert!(!queue.follows_presses());
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 3
        );
    }

    /// LVGL never ends these presses: an encoder long press that leaves edit
    /// mode, and a keypad whose focus moved to another window. Once the window
    /// sees the devices released, it gives their reservations back.
    #[test]
    fn presses_of_released_devices_give_their_reservations_back() {
        let mut queue = EventQueue::new();
        let (pointer, keypad, encoder) = (device(1), device(2), device(3));
        queue.push(event(EventKind::Pressed, 1), encoder);
        queue.push(event(EventKind::Pressed, 2), keypad);
        queue.push(event(EventKind::Pressed, 3), pointer);
        // Only four slots are left, and two reservations are stale: a press of
        // a fourth device would be rejected.
        let reserved = 3 * PRESS_END_RESERVATION as usize;
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 3 - reserved
        );
        assert_eq!(
            queue.push(event(EventKind::Pressed, 4), device(4)),
            PushOutcome::Dropped
        );
        drain(&mut queue);
        assert_eq!(queue.followed_presses().len(), 4);

        // The pointer is still pressed.
        queue.forget_ended_presses(|source| source == pointer);
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(queue.followed_presses()[0].device, pointer);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - PRESS_END_RESERVATION as usize
        );
        for code in SHORT_PRESS_END {
            assert_ne!(
                queue.push(event(code, 3), pointer),
                PushOutcome::Dropped,
                "{code:?}"
            );
        }
        assert!(!queue.follows_presses());
    }

    /// A released device has already sent the end of its press, so forgetting
    /// a rejected press never lets its end through.
    #[test]
    fn forgetting_ended_presses_keeps_live_rejected_ones() {
        let mut queue = EventQueue::new();
        let keypad = device(2);
        flood(&mut queue, EventKind::Key, 9);
        assert_eq!(
            queue.push(event(EventKind::Pressed, 1), keypad),
            PushOutcome::Dropped
        );
        drain(&mut queue);
        // Still held: the rejected press stays followed.
        queue.forget_ended_presses(|source| source == keypad);
        for code in SHORT_PRESS_END {
            assert_eq!(
                queue.push(event(code, 2), keypad),
                PushOutcome::Dropped,
                "{code:?}"
            );
        }
        assert!(!queue.follows_presses());

        // Released without an end for this window, then forgotten.
        flood(&mut queue, EventKind::Key, 9);
        queue.push(event(EventKind::Pressed, 1), keypad);
        drain(&mut queue);
        queue.forget_ended_presses(|_| false);
        assert!(!queue.follows_presses());
    }

    #[test]
    fn a_device_reset_forgets_its_press() {
        let mut queue = EventQueue::new();
        let (pointer, keypad) = (device(1), device(2));
        queue.push(event(EventKind::Pressed, 1), pointer);
        queue.push(event(EventKind::Pressed, 2), keypad);
        assert_eq!(
            queue.push(event(EventKind::InputDeviceReset, 1), pointer),
            PushOutcome::Queued
        );
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(queue.followed_presses()[0].device, keypad);
        // A reset sent by code forgets nothing.
        queue.push(event(EventKind::InputDeviceReset, 2), ptr::null());
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Pressed, 1),
                (EventKind::Pressed, 2),
                (EventKind::InputDeviceReset, 1),
                (EventKind::InputDeviceReset, 2),
            ]
        );
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - PRESS_END_RESERVATION as usize
        );
    }

    /// The consumer discards a widget while its keypad press is queued, and
    /// the keypad then releases on another widget: the end must not arrive
    /// without its `Pressed`.
    #[test]
    fn discarding_a_target_keeps_its_press_events() {
        let mut queue = EventQueue::new();
        let keypad = device(2);
        queue.push(event(EventKind::Pressed, 1), keypad);
        queue.push(pointer_move(1, 3), keypad);
        queue.push(event(EventKind::ValueChanged, 1), keypad);
        queue.push(event(EventKind::Focused, 1), ptr::null());
        queue.push(event(EventKind::Clicked, 3), ptr::null());
        queue.discard_target(object(1));
        assert!(!queue.follows_presses(), "the reservation is given back");
        for code in SHORT_PRESS_END {
            assert_eq!(
                queue.push(event(code, 2), keypad),
                PushOutcome::Queued,
                "{code:?}"
            );
        }
        let mut expected = Vec::from([(EventKind::Pressed, 0), (EventKind::Clicked, 3)]);
        expected.extend(SHORT_PRESS_END.map(|code| (code, 2)));
        assert_eq!(drain(&mut queue), expected);

        // The ends of a press on the discarded widget are kept as well.
        queue.push(event(EventKind::Pressed, 1), keypad);
        queue.push(event(EventKind::LongPressed, 1), keypad);
        queue.push(event(EventKind::Released, 1), keypad);
        queue.push(event(EventKind::LongPressedRepeat, 1), keypad);
        queue.discard_target(object(1));
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Pressed, 0),
                (EventKind::LongPressed, 0),
                (EventKind::Released, 0),
            ]
        );
    }

    /// Latest wins is matched by kind and target only: two devices moving on
    /// one widget, or changing its value, merge into the latest event.
    #[test]
    fn two_devices_on_one_widget_share_the_latest_move_and_value() {
        let mut queue = EventQueue::new();
        let (pointer, keypad) = (device(1), device(2));
        queue.push(event(EventKind::Pressed, 1), pointer);
        queue.push(event(EventKind::Pressed, 1), keypad);
        queue.push(pointer_move(1, 4), pointer);
        assert_eq!(
            queue.push(pointer_move(1, 8), keypad),
            PushOutcome::Coalesced
        );
        queue.push(event(EventKind::ValueChanged, 1), pointer);
        assert_eq!(
            queue.push(event(EventKind::ValueChanged, 1), keypad),
            PushOutcome::Coalesced
        );
        queue.push(event(EventKind::LongPressedRepeat, 1), pointer);
        assert_eq!(
            queue.push(event(EventKind::LongPressedRepeat, 1), keypad),
            PushOutcome::Coalesced
        );
        let drained: Vec<_> = core::iter::from_fn(|| queue.pop_front())
            .map(|event| (event.code, event.pointer))
            .collect();
        assert_eq!(
            drained,
            [
                (EventKind::Pressed, None),
                (EventKind::Pressed, None),
                (EventKind::Pressing, Some([8, 0, 100, 100])),
                (EventKind::ValueChanged, None),
                (EventKind::LongPressedRepeat, None),
            ]
        );
    }

    #[test]
    fn forgetting_a_target_forgets_its_accepted_press() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1), device(1));
        queue.discard_target(object(1));
        assert!(!queue.follows_presses());

        queue.push(event(EventKind::Pressed, 2), device(1));
        queue.clear_target(object(2));
        assert!(!queue.follows_presses());

        queue.push(event(EventKind::Pressed, 3), device(1));
        queue.push(event(EventKind::Pressed, 4), device(2));
        queue.clear_dead_targets(|target| target == object(4));
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(queue.followed_presses()[0].target, object(4));
        drain(&mut queue);

        // A rejected press is kept: its device may end it on another widget.
        flood(&mut queue, EventKind::Key, 9);
        assert_eq!(
            queue.push(event(EventKind::Pressed, 5), device(3)),
            PushOutcome::Dropped
        );
        drain(&mut queue);
        queue.discard_target(object(5));
        queue.clear_dead_targets(|target| target != object(5));
        assert_eq!(
            queue.push(event(EventKind::Released, 6), device(3)),
            PushOutcome::Dropped
        );
    }

    #[test]
    fn losses_are_reported_at_most_once_per_interval() {
        let mut queue = EventQueue::new();
        assert_eq!(queue.take_loss_report(0), None);
        flood(&mut queue, EventKind::Key, 1);
        queue.send(event(EventKind::Key, 1));
        assert!(queue.has_unreported_losses());
        assert_eq!(
            queue.take_loss_report(5_000),
            Some(LossReport { lost: 2, total: 2 })
        );
        assert!(!queue.has_unreported_losses());

        // The consumer never drains the queue: losses keep being reported.
        for now in [5_100, 5_900] {
            queue.send(event(EventKind::Key, 1));
            assert_eq!(queue.take_loss_report(now), None);
        }
        queue.send(event(EventKind::Key, 1));
        assert_eq!(
            queue.take_loss_report(6_000),
            Some(LossReport { lost: 3, total: 5 })
        );
        assert_eq!(queue.take_loss_report(60_000), None, "nothing new lost");

        // Ticks wrap around.
        let mut queue = EventQueue::new();
        flood(&mut queue, EventKind::Key, 1);
        assert!(queue.take_loss_report(u32::MAX - 10).is_some());
        queue.send(event(EventKind::Key, 1));
        assert_eq!(queue.take_loss_report(100), None);
        assert_eq!(
            queue.take_loss_report(LOSS_REPORT_INTERVAL - 11),
            Some(LossReport { lost: 1, total: 2 })
        );
        assert_eq!(queue.dropped(), 2);
    }

    /// Deterministic pseudo-random numbers (xorshift64*).
    struct Random(u64);

    impl Random {
        fn below(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % bound
        }
    }

    /// One input device and its current press: target, press number and
    /// whether `LongPressed` was sent.
    struct Device {
        identifier: usize,
        first_target: usize,
        targets: usize,
        /// Whether it may release on another widget, like a keypad.
        releases_elsewhere: bool,
        press: Option<(usize, usize, bool)>,
        abandoned: bool,
    }

    /// A queue fed by the randomized test, which checks every push against
    /// its outcome. Each event carries its sequence number and its press
    /// number (0 outside a device press) in `pointer`.
    struct Stream {
        queue: EventQueue,
        sequence: i32,
        /// Per sequence number: whether the event is lifecycle and queued, so
        /// the consumer must receive it.
        must_arrive: Vec<bool>,
    }

    impl Stream {
        fn tier_count(&self, wanted: Tier) -> usize {
            (0..self.queue.len())
                .filter(|&index| {
                    self.queue
                        .get(index)
                        .is_some_and(|event| tier(event.code) == wanted)
                })
                .count()
        }

        fn sequences(&self) -> Vec<i32> {
            (0..self.queue.len())
                .filter_map(|index| self.queue.get(index))
                .map(|event| event.pointer.unwrap()[0])
                .collect()
        }

        fn push(&mut self, code: EventKind, target: usize, press: usize, source: usize) {
            self.sequence += 1;
            let mut queued = event(code, target);
            queued.pointer = Some([self.sequence, press as i32, 0, 0]);
            let (length, dropped) = (self.queue.len(), self.queue.dropped());
            let state = self.tier_count(Tier::State);
            let outcome = self.queue.push(queued, device(source));

            let back = self
                .queue
                .len()
                .checked_sub(1)
                .and_then(|index| self.queue.get(index).map(|event| event.pointer.unwrap()[0]));
            let (expected_length, expected_dropped) = match outcome {
                PushOutcome::Queued => (length + 1, dropped),
                PushOutcome::Coalesced => (length, dropped),
                PushOutcome::EvictedOldest => {
                    assert_eq!(length, WINDOW_EVENT_QUEUE_CAPACITY, "evicted from room");
                    (length, dropped + 1)
                }
                PushOutcome::Dropped => (length, dropped + 1),
            };
            assert_eq!(self.queue.len(), expected_length, "{code:?} {outcome:?}");
            assert_eq!(
                self.queue.dropped(),
                expected_dropped,
                "{code:?} {outcome:?}"
            );
            assert_eq!(
                back == Some(self.sequence),
                outcome != PushOutcome::Dropped,
                "{code:?} {outcome:?}"
            );
            if outcome == PushOutcome::Coalesced {
                assert!(tier(code).coalesces(), "{code:?} coalesced");
            }
            if tier(code) < Tier::State {
                assert!(
                    self.tier_count(Tier::State) >= state,
                    "{code:?} pushed out a state event"
                );
            }
            self.must_arrive
                .push(tier(code) == Tier::Lifecycle && outcome != PushOutcome::Dropped);
        }

        /// The widget `target` is deleted: the window forgets it in one of
        /// the three ways it can.
        fn delete(&mut self, target: usize, way: usize) {
            let before = self.sequences();
            match way {
                0 => self.queue.discard_target(object(target)),
                1 => self.queue.clear_target(object(target)),
                _ => self
                    .queue
                    .clear_dead_targets(|candidate| candidate != object(target)),
            }
            let after = self.sequences();
            for sequence in before {
                if !after.contains(&sequence) {
                    self.must_arrive[sequence as usize - 1] = false;
                }
            }
        }
    }

    /// Random LVGL-like streams from a pointer, a keypad and an encoder,
    /// with random consumer pauses. Presses may be left without an end, two
    /// devices press the same widgets, the keypad may release on another
    /// widget, widgets are deleted and devices reset, code sends press events
    /// without a device, and the window forgets the presses of released
    /// devices. The consumer never receives the end of a press whose
    /// `Pressed` was dropped, events keep their order, value changes stay
    /// inside their press, an accepted press arrives whole unless its widget
    /// was deleted, every queued lifecycle event arrives, and every push
    /// reports what it did.
    #[test]
    fn random_bursts_keep_presses_whole_and_in_order() {
        let mut random = Random(0x9E37_79B9_7F4A_7C15);
        let mut stream = Stream {
            queue: EventQueue::new(),
            sequence: 0,
            must_arrive: Vec::new(),
        };
        let mut devices = [(1, 1, 4, false), (2, 3, 4, true), (3, 5, 3, false)].map(
            |(identifier, first_target, targets, releases_elsewhere)| Device {
                identifier,
                first_target,
                targets,
                releases_elsewhere,
                press: None,
                abandoned: false,
            },
        );
        let mut delivered = Vec::new();
        // Per press: its device, then the events that hold a reservation,
        // pushed and delivered, and whether its widget was deleted during it.
        let mut presses: Vec<(usize, usize, usize, bool)> = Vec::new();
        let mut replacements = 0;
        let mut deletions = 0;

        for _ in 0..300_000 {
            match random.below(20) {
                0..=9 => {
                    let device = &mut devices[random.below(3)];
                    let source = device.identifier;
                    match device.press {
                        None => {
                            let target = device.first_target + random.below(device.targets);
                            presses.push((source, 0, 0, false));
                            let press = presses.len();
                            replacements += usize::from(core::mem::take(&mut device.abandoned));
                            device.press = Some((target, press, false));
                            stream.push(EventKind::Pressed, target, press, source);
                        }
                        Some((target, press, long)) => match random.below(15) {
                            0..=5 => stream.push(EventKind::Pressing, target, press, source),
                            6 => stream.push(EventKind::ValueChanged, target, press, source),
                            7 if !long => {
                                stream.push(EventKind::LongPressed, target, press, source);
                                presses[press - 1].1 += 1;
                                device.press = Some((target, press, true));
                            }
                            7 => stream.push(EventKind::LongPressedRepeat, target, press, source),
                            8 => {
                                stream.push(EventKind::PressLost, target, press, source);
                                presses[press - 1].1 += 1;
                                device.press = None;
                            }
                            9 => {
                                // Released after a scroll: no click.
                                stream.push(EventKind::Released, target, press, source);
                                presses[press - 1].1 += 1;
                                device.press = None;
                            }
                            10 => {
                                // No end at all: an encoder long press that
                                // leaves edit mode.
                                device.press = None;
                                device.abandoned = true;
                            }
                            11 if !device.releases_elsewhere => {
                                // LVGL forgets the pressed widget: the press
                                // never ends.
                                stream.push(EventKind::InputDeviceReset, target, 0, source);
                                device.press = None;
                                device.abandoned = true;
                            }
                            _ => {
                                let target = if device.releases_elsewhere && random.below(2) == 0 {
                                    device.first_target + random.below(device.targets)
                                } else {
                                    target
                                };
                                // LVGL sends a release and its clicks back to back.
                                let mut codes = Vec::from([EventKind::Released]);
                                if !long {
                                    codes.push(EventKind::ShortClicked);
                                    codes.push(
                                        [
                                            EventKind::SingleClicked,
                                            EventKind::DoubleClicked,
                                            EventKind::TripleClicked,
                                        ][random.below(3)],
                                    );
                                }
                                codes.push(EventKind::Clicked);
                                for code in codes {
                                    stream.push(code, target, press, source);
                                    presses[press - 1].1 += 1;
                                }
                                device.press = None;
                            }
                        },
                    }
                }
                10 | 11 => stream.push(
                    [EventKind::Key, EventKind::Focused, EventKind::Custom1][random.below(3)],
                    10 + random.below(3),
                    0,
                    0,
                ),
                12 | 13 => stream.push(
                    [
                        EventKind::Scroll,
                        EventKind::ScrollEnd,
                        EventKind::ScrollBegin,
                    ][random.below(3)],
                    1 + random.below(12),
                    0,
                    0,
                ),
                14 | 15 => stream.push(
                    [EventKind::ValueChanged, EventKind::SizeChanged][random.below(2)],
                    20 + random.below(2),
                    0,
                    0,
                ),
                16 => {
                    // Press events sent by code, without a device.
                    let code = [
                        EventKind::Pressed,
                        EventKind::Pressing,
                        EventKind::Released,
                        EventKind::Clicked,
                        EventKind::PressLost,
                    ][random.below(5)];
                    stream.push(code, 1 + random.below(7), 0, 0);
                }
                17 if random.below(20) == 0 => {
                    // A widget is deleted, and a new one may take its
                    // address. LVGL forgets a press of that widget, except a
                    // keypad, which may end it on the next focused widget.
                    let target = 1 + random.below(21);
                    for device in &mut devices {
                        let Some((pressed, press, long)) = device.press else {
                            continue;
                        };
                        if pressed != target {
                            continue;
                        }
                        presses[press - 1].3 = true;
                        if device.releases_elsewhere && random.below(2) == 0 {
                            let next = device.first_target + random.below(device.targets);
                            device.press = Some((next, press, long));
                        } else {
                            device.press = None;
                            device.abandoned = true;
                        }
                    }
                    stream.delete(target, random.below(3));
                    deletions += 1;
                }
                _ => {
                    if random.below(4) != 0 {
                        // The window forgets the presses of released devices
                        // when the consumer pops.
                        if random.below(2) == 0 {
                            stream.queue.forget_ended_presses(|source| {
                                devices.iter().any(|device| {
                                    device.press.is_some() && source == device_of(device)
                                })
                            });
                        }
                        delivered.extend(stream.queue.pop_front());
                    }
                }
            }
        }
        delivered.extend(core::iter::from_fn(|| stream.queue.pop_front()));
        assert!(stream.queue.dropped() > 10_000, "the stream must overflow");
        assert!(
            replacements > 1_000,
            "presses without an end must be replaced"
        );
        assert!(deletions > 100, "widgets must be deleted");

        // Per device: the press being delivered and whether it was released.
        let mut state = [None::<(usize, bool)>; 4];
        let mut ended = alloc::vec![false; presses.len() + 1];
        let mut arrived = alloc::vec![false; stream.must_arrive.len()];
        let mut last_sequence = 0;
        let mut accepted = 0;
        for event in &delivered {
            let [sequence, press, _, _] = event.pointer.unwrap();
            let press = press as usize;
            assert!(sequence > last_sequence, "events were reordered");
            last_sequence = sequence;
            arrived[sequence as usize - 1] = true;
            if press == 0 {
                continue;
            }
            let source = presses[press - 1].0;
            let Some(role) = press_role(event.code) else {
                assert_eq!(event.code, EventKind::ValueChanged);
                assert!(!ended[press], "a value change crossed the end of its press");
                continue;
            };
            let current = state[source];
            match role {
                PressRole::Start => {
                    accepted += 1;
                    state[source] = Some((press, false));
                }
                PressRole::Progress | PressRole::LongPress | PressRole::Lost => {
                    assert_eq!(
                        current,
                        Some((press, false)),
                        "{:?} without its press",
                        event.code
                    );
                }
                PressRole::Release => {
                    assert_eq!(current, Some((press, false)), "release without its press");
                    state[source] = Some((press, true));
                }
                PressRole::Click | PressRole::Final => {
                    assert_eq!(
                        current,
                        Some((press, true)),
                        "{:?} without its release",
                        event.code
                    );
                }
            }
            if matches!(
                role,
                PressRole::Lost | PressRole::Release | PressRole::Final
            ) {
                ended[press] = true;
            }
            if matches!(role, PressRole::Lost | PressRole::Final) {
                state[source] = None;
            }
            if role != PressRole::Start && role != PressRole::Progress {
                presses[press - 1].2 += 1;
            }
        }
        assert!(accepted > 1_000, "presses must get through");
        for event in &delivered {
            let press = event.pointer.unwrap()[1] as usize;
            if event.code == EventKind::Pressed && press > 0 {
                let (_, pushed, received, deleted) = presses[press - 1];
                if !deleted {
                    assert_eq!(pushed, received, "press {press} arrived incomplete");
                }
            }
        }
        for (index, must_arrive) in stream.must_arrive.iter().enumerate() {
            assert!(
                !must_arrive || arrived[index],
                "queued lifecycle event {} was lost",
                index + 1
            );
        }
    }

    fn device_of(model: &Device) -> InputDevice {
        device(model.identifier)
    }

    #[test]
    fn key_information_is_captured_with_the_event() {
        let mut queue = EventQueue::new();
        queue.send(Event::new(EventKind::Key, object(1), Some(Key::Escape)));
        assert_eq!(queue.pop_front().unwrap().key, Some(Key::Escape));
    }

    #[test]
    fn discarding_a_target_preserves_the_order_of_the_others() {
        let mut queue = EventQueue::new();
        // Wrap the ring so the compaction crosses the end of the array.
        for _ in 0..12 {
            queue.send(event(EventKind::Clicked, 7));
        }
        for _ in 0..12 {
            queue.pop_front();
        }
        for (index, target) in [1, 2, 1, 3, 2, 1, 3, 3].into_iter().enumerate() {
            queue.send(event(
                if index % 2 == 0 {
                    EventKind::Focused
                } else {
                    EventKind::Defocused
                },
                target,
            ));
        }
        queue.discard_target(object(1));
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Defocused, 2),
                (EventKind::Defocused, 3),
                (EventKind::Focused, 2),
                (EventKind::Focused, 3),
                (EventKind::Defocused, 3),
            ]
        );
        assert!(queue.is_empty());
        // The ring is fully usable afterwards.
        for index in 0..WINDOW_EVENT_QUEUE_CAPACITY {
            queue.send(event(EventKind::Clicked, index + 1));
        }
        assert!(queue.is_full());
    }

    #[test]
    fn clearing_a_target_keeps_events_but_nulls_the_target() {
        let mut queue = EventQueue::new();
        queue.send(event(EventKind::Pressed, 1));
        queue.send(event(EventKind::Clicked, 2));
        queue.send(event(EventKind::Released, 1));
        queue.clear_target(object(1));
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Pressed, 0),
                (EventKind::Clicked, 2),
                (EventKind::Released, 0),
            ]
        );
    }

    #[test]
    fn dead_targets_are_cleared_and_live_ones_kept() {
        let mut queue = EventQueue::new();
        for target in [1, 1, 2, 3, 2, 1] {
            queue.send(event(EventKind::Clicked, target));
        }
        let mut calls = 0;
        queue.clear_dead_targets(|target| {
            calls += 1;
            target == object(2)
        });
        // One call per run of equal targets, not per event.
        assert_eq!(calls, 5);
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Clicked, 0),
                (EventKind::Clicked, 0),
                (EventKind::Clicked, 2),
                (EventKind::Clicked, 0),
                (EventKind::Clicked, 2),
                (EventKind::Clicked, 0),
            ]
        );
    }

    #[test]
    fn queue_storage_is_fixed_and_fits_the_window_class() {
        extern crate std;
        use alloc::collections::VecDeque;
        std::println!(
            "Event={} Option<Event>={} PressGroup={} old queue={}+{} new queue={} Window={}",
            size_of::<Event>(),
            size_of::<Option<Event>>(),
            size_of::<PressGroup>(),
            size_of::<VecDeque<Event>>(),
            10 * size_of::<Event>(),
            size_of::<EventQueue>(),
            size_of::<crate::Window>()
        );
        assert!(size_of::<crate::Window>() < 65536);
        assert!(
            size_of::<EventQueue>()
                <= WINDOW_EVENT_QUEUE_CAPACITY * size_of::<Option<Event>>()
                    + PRESS_TRACKING_CAPACITY * size_of::<PressGroup>()
                    + 24
        );
    }

    #[test]
    fn a_burst_never_grows_the_ring() {
        // Constructible in a constant: there is no allocation to perform.
        const EMPTY: EventQueue = EventQueue::new();
        let mut queue = EMPTY;
        let size = size_of::<EventQueue>();
        for index in 0..10_000usize {
            let code = match index % 5 {
                0 => EventKind::Pressed,
                1 | 2 => EventKind::Pressing,
                3 => EventKind::Key,
                _ => EventKind::Released,
            };
            queue.send(event(code, index % 3 + 1));
            assert!(queue.len() <= WINDOW_EVENT_QUEUE_CAPACITY);
            assert!(queue.followed_presses().len() <= PRESS_TRACKING_CAPACITY);
            if index % 7 == 0 {
                queue.pop_front();
            }
        }
        assert_eq!(size_of::<EventQueue>(), size);
        assert!(queue.dropped() > 0);
    }
}
