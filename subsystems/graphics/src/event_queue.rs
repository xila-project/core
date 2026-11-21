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

/// Accepted presses that may hold a reservation at once: one pointer press and
/// one keypad or encoder press.
const MAXIMUM_ACCEPTED_PRESSES: usize = 2;

/// Press groups, accepted or rejected, that the queue follows at once.
const PRESS_TRACKING_CAPACITY: usize = 4;

/// Minimum time between two loss reports, in LVGL ticks (milliseconds).
pub const LOSS_REPORT_INTERVAL: u32 = 1000;

// Indices are stored as `u8`.
const _: () =
    assert!(WINDOW_EVENT_QUEUE_CAPACITY > 0 && WINDOW_EVENT_QUEUE_CAPACITY <= u8::MAX as usize);
// Every accepted press fits with its whole end, and room remains for others.
const _: () = assert!(
    MAXIMUM_ACCEPTED_PRESSES * (PRESS_END_RESERVATION as usize + 1) < WINDOW_EVENT_QUEUE_CAPACITY
);
const _: () = assert!(MAXIMUM_ACCEPTED_PRESSES < PRESS_TRACKING_CAPACITY);

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

/// How much an event may be sacrificed when the queue is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    /// Notifications no consumer depends on: evicted first.
    Low,
    /// "Latest wins" events: a newer one for the same target replaces the
    /// queued one, and they are evicted after low events.
    Coalescible,
    /// Input, focus and window lifecycle events: never evicted, never reordered.
    Lifecycle,
}

const fn tier(kind: EventKind) -> Tier {
    match kind {
        EventKind::Scroll
        | EventKind::ScrollBegin
        | EventKind::ScrollThrowBegin
        | EventKind::ScrollEnd
        | EventKind::StyleChanged
        | EventKind::SizeChanged
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
        EventKind::Pressing | EventKind::ValueChanged | EventKind::LongPressedRepeat => {
            Tier::Coalescible
        }
        _ => Tier::Lifecycle,
    }
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

/// A press the queue follows from its `Pressed` event to its end.
#[derive(Debug, Clone, Copy)]
struct PressGroup {
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
        target: ptr::null_mut(),
        accepted: false,
        released: false,
        reserved: 0,
    };
}

/// Bounded queue of window events in arrival order.
///
/// Events fall in three tiers:
///
/// - low: the scroll family, style, size and layout changes, hover, hit tests
///   and drawing notifications;
/// - coalescible: `Pressing`, `ValueChanged` and `LongPressedRepeat`, where
///   only the latest matters. A newer one replaces the queued event of the
///   same kind and target and moves to the back, unless a lifecycle event for
///   that target lies between them;
/// - lifecycle: everything else, such as presses, releases, clicks, keys,
///   focus changes, `Ready`, `Cancel`, `Delete` and `CloseRequested`.
///
/// When the queue is full, an arrival evicts the oldest low event, or else,
/// unless it is itself low, the oldest coalescible event. Lifecycle events are
/// never evicted or reordered. An arrival that finds nothing to evict is
/// rejected.
///
/// Presses are kept whole: a `Pressed` event is accepted only with room for
/// the rest of its press (`LongPressed`, `Released` or `PressLost`, then the
/// clicks), which stays reserved until those events arrive. Other lifecycle events cannot take that room. When
/// a `Pressed` event is rejected, the rest of that press is dropped as well,
/// so a consumer never receives a release or a click without its press.
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

    /// Append an event according to the queue's overflow policy.
    pub fn push(&mut self, event: Event) -> PushOutcome {
        let Some(role) = press_role(event.code) else {
            return self.insert(event, false);
        };
        if role == PressRole::Start {
            return self.push_press(event);
        }
        let Some(index) = self.press_index(event.target) else {
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

    fn push_press(&mut self, event: Event) -> PushOutcome {
        // LVGL sends a release and its clicks back to back, so a new press
        // means every released press has ended. A new press of the same
        // target ends its previous one.
        self.forget_presses(|group| group.released || group.target == event.target);
        // A third accepted press means one of them lost its end (its widget
        // was disabled while pressed, for example). Forgetting an accepted
        // press only gives up its reservation.
        while self.accepted_press_count() >= MAXIMUM_ACCEPTED_PRESSES {
            let oldest = self.presses[..self.press_count as usize]
                .iter()
                .position(|group| group.accepted);
            match oldest {
                Some(index) => self.forget_press(index),
                None => break,
            }
        }

        let accepted =
            self.lifecycle_count() + 1 + self.reserved() + PRESS_END_RESERVATION as usize
                <= WINDOW_EVENT_QUEUE_CAPACITY;
        self.follow_press(PressGroup {
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
        if tier == Tier::Coalescible
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

        let victim = self.oldest(Tier::Low).or_else(|| {
            if tier == Tier::Low {
                None
            } else {
                self.oldest(Tier::Coalescible)
            }
        });
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

    /// Remove every event aimed at `target`, preserving the order of the rest,
    /// and stop following its press.
    /// Only compares addresses; never dereferences `target`.
    pub fn discard_target(&mut self, target: *mut lvgl::lv_obj_t) {
        let mut kept = 0;
        for index in 0..self.len() {
            let source = self.physical(index);
            let Some(event) = self.slots[source].take() else {
                continue;
            };
            if event.target != target {
                let destination = self.physical(kept);
                self.slots[destination] = Some(event);
                kept += 1;
            }
        }
        self.length = kept as u8;
        self.forget_presses(|group| group.target == target);
    }

    /// Keep every event but null the ones aimed at `target`, and stop
    /// following its press.
    /// Only compares addresses; never dereferences `target`.
    pub fn clear_target(&mut self, target: *mut lvgl::lv_obj_t) {
        for event in self.slots.iter_mut().flatten() {
            if event.target == target {
                event.target = ptr::null_mut();
            }
        }
        self.forget_presses(|group| group.target == target);
    }

    /// Null the target of every event whose target `is_live` rejects, keep
    /// the events, and stop following presses of rejected targets. `is_live`
    /// is called once per distinct run of targets and once per followed press,
    /// and must only compare the address; the targets may already be deleted.
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
        self.forget_presses(|group| !is_live(group.target));
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

    fn accepted_press_count(&self) -> usize {
        self.followed_presses()
            .iter()
            .filter(|group| group.accepted)
            .count()
    }

    fn followed_presses(&self) -> &[PressGroup] {
        &self.presses[..self.press_count as usize]
    }

    fn press_index(&self, target: *mut lvgl::lv_obj_t) -> Option<usize> {
        self.followed_presses()
            .iter()
            .position(|group| group.target == target)
    }

    /// Start following a press, making room by forgetting the oldest accepted
    /// press, or else the oldest press. Only the latter can let the end of a
    /// rejected press through, and it takes more simultaneous rejected presses
    /// than an input device can produce.
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
        while queue.push(event(code, target)) != PushOutcome::Dropped {
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
                    queue.push(event(EventKind::Clicked, round * 100 + index + 1)),
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
        queue.push(event(EventKind::Pressed, 1));
        queue.push(event(EventKind::Released, 1));
        assert_eq!(queue.front().unwrap().code, EventKind::Pressed);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop_front().unwrap().code, EventKind::Pressed);
        assert_eq!(queue.front().unwrap().code, EventKind::Released);
    }

    #[test]
    fn pointer_moves_for_one_target_keep_the_latest() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        assert_eq!(queue.push(pointer_move(1, 10)), PushOutcome::Queued);
        for x in 11..200 {
            assert_eq!(queue.push(pointer_move(1, x)), PushOutcome::Coalesced);
        }
        assert_eq!(queue.len(), 2);
        queue.pop_front();
        assert_eq!(queue.pop_front().unwrap().pointer, Some([199, 0, 100, 100]));
        assert_eq!(queue.dropped(), 0);
    }

    #[test]
    fn coalescing_moves_the_latest_event_to_the_back_even_when_not_at_the_tail() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        queue.push(pointer_move(1, 1));
        queue.push(event(EventKind::ValueChanged, 1));
        queue.push(event(EventKind::Scroll, 1));
        queue.push(pointer_move(2, 5));
        assert_eq!(queue.push(pointer_move(1, 2)), PushOutcome::Coalesced);
        assert_eq!(
            queue.push(event(EventKind::ValueChanged, 1)),
            PushOutcome::Coalesced
        );
        assert_eq!(
            queue.push(event(EventKind::LongPressedRepeat, 1)),
            PushOutcome::Queued
        );
        assert_eq!(
            queue.push(event(EventKind::LongPressedRepeat, 1)),
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
        queue.push(pointer_move(1, 1));
        queue.push(event(EventKind::Key, 1));
        assert_eq!(queue.push(pointer_move(1, 2)), PushOutcome::Queued);
        queue.push(event(EventKind::ValueChanged, 2));
        queue.push(event(EventKind::Clicked, 3));
        assert_eq!(
            queue.push(event(EventKind::ValueChanged, 2)),
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
    fn full_queue_evicts_low_events_first_then_coalescible_ones() {
        let mut queue = EventQueue::new();
        for address in 1..=8 {
            queue.push(event(EventKind::ValueChanged, address));
            queue.push(event(EventKind::ScrollEnd, address));
        }
        assert!(queue.is_full());
        // Low arrivals only replace low events.
        assert_eq!(
            queue.push(event(EventKind::StyleChanged, 9)),
            PushOutcome::EvictedOldest
        );
        for address in 10..=17 {
            assert_eq!(
                queue.push(event(EventKind::Focused, address)),
                PushOutcome::EvictedOldest
            );
        }
        // Every low event is gone: coalescible events are evicted next, and a
        // low arrival is rejected.
        assert_eq!(
            queue.push(event(EventKind::ScrollBegin, 9)),
            PushOutcome::Dropped
        );
        assert_eq!(
            queue.push(event(EventKind::Defocused, 18)),
            PushOutcome::EvictedOldest
        );
        let drained = drain(&mut queue);
        assert_eq!(
            drained[..7],
            (2..=8)
                .map(|address| (EventKind::ValueChanged, address))
                .collect::<Vec<_>>()
        );
        assert_eq!(drained[7], (EventKind::Focused, 10));
        assert_eq!(*drained.last().unwrap(), (EventKind::Defocused, 18));
        assert_eq!(queue.dropped(), 11);
    }

    #[test]
    fn lifecycle_events_are_never_evicted() {
        let mut queue = EventQueue::new();
        for address in 1..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.push(event(EventKind::Clicked, address));
        }
        for code in [
            EventKind::Clicked,
            EventKind::Pressing,
            EventKind::ValueChanged,
            EventKind::Scroll,
            EventKind::CloseRequested,
        ] {
            assert_eq!(queue.push(event(code, 99)), PushOutcome::Dropped);
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
    fn scroll_family_and_layout_events_are_low_priority() {
        for code in [
            EventKind::Scroll,
            EventKind::ScrollBegin,
            EventKind::ScrollThrowBegin,
            EventKind::ScrollEnd,
            EventKind::StyleChanged,
            EventKind::SizeChanged,
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
            queue.push(event(EventKind::Pressed, 1)),
            PushOutcome::Queued
        );
        for step in 0..512 {
            queue.push(pointer_move(1, step));
            queue.push(event(EventKind::ValueChanged, 1));
            queue.push(event(EventKind::Scroll, 3));
            queue.push(event(EventKind::Clicked, 2));
        }
        for code in SHORT_PRESS_END {
            assert_ne!(queue.push(event(code, 1)), PushOutcome::Dropped, "{code:?}");
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
        queue.push(event(EventKind::Pressed, 1));
        queue.push(event(EventKind::Focused, 2));
        for step in 0..300 {
            if step == 3 {
                assert_eq!(
                    queue.push(event(EventKind::LongPressed, 1)),
                    PushOutcome::Queued
                );
            }
            queue.push(pointer_move(1, step));
            queue.push(event(EventKind::LongPressedRepeat, 1));
            queue.push(Event::new(
                EventKind::Key,
                object(2),
                Some(Key::Character(b'a')),
            ));
            queue.push(event(EventKind::ValueChanged, 2));
        }
        for code in [EventKind::Released, EventKind::Clicked] {
            assert_ne!(queue.push(event(code, 1)), PushOutcome::Dropped, "{code:?}");
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
            queue.push(event(EventKind::Pressed, 1)),
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
            assert_eq!(queue.push(event(code, 1)), PushOutcome::Dropped, "{code:?}");
        }
        // A press lost to a slide onto another widget is dropped too.
        assert_eq!(
            queue.push(event(EventKind::Pressed, 2)),
            PushOutcome::Dropped
        );
        assert_eq!(
            queue.push(event(EventKind::PressLost, 2)),
            PushOutcome::Dropped
        );
        let drained = drain(&mut queue);
        assert!(only(&drained, 1).is_empty());
        assert!(only(&drained, 2).is_empty());
        assert_eq!(queue.dropped(), 1 + 1 + 7 + 2);

        // The next press of the same target is accepted again.
        assert_eq!(
            queue.push(event(EventKind::Pressed, 1)),
            PushOutcome::Queued
        );
        assert_eq!(
            queue.push(event(EventKind::Released, 1)),
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
        queue.push(event(EventKind::Pressed, 1));
        let room = flood(&mut queue, EventKind::Key, 9);
        assert_eq!(
            room,
            WINDOW_EVENT_QUEUE_CAPACITY - 1 - PRESS_END_RESERVATION as usize
        );
        for code in SHORT_PRESS_END {
            assert_eq!(queue.push(event(code, 1)), PushOutcome::Queued, "{code:?}");
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
        queue.push(event(EventKind::Pressed, 1));
        for address in 2..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.push(pointer_move(address, 0));
        }
        assert!(queue.is_full());
        for code in SHORT_PRESS_END {
            assert_eq!(
                queue.push(event(code, 1)),
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
        queue.push(event(EventKind::Pressed, 1));
        queue.push(event(EventKind::PressLost, 1));
        assert!(!queue.follows_presses());
        queue.push(event(EventKind::Pressed, 2));
        queue.push(event(EventKind::LongPressed, 2));
        queue.push(event(EventKind::Released, 2));
        queue.push(event(EventKind::Clicked, 2));
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
        queue.push(event(EventKind::Pressed, 1));
        queue.push(event(EventKind::Released, 1));
        assert_eq!(drain(&mut queue).len(), 2);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY - 3
        );
        drain(&mut queue);
        queue.push(event(EventKind::Pressed, 2));
        queue.push(event(EventKind::PressLost, 2));
        assert!(!queue.follows_presses());
        drain(&mut queue);
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY
        );
    }

    #[test]
    fn presses_that_never_end_hold_at_most_two_reservations() {
        let mut queue = EventQueue::new();
        for address in 1..=5 {
            assert_eq!(
                queue.push(event(EventKind::Pressed, address)),
                PushOutcome::Queued
            );
        }
        assert_eq!(
            flood(&mut queue, EventKind::Key, 9),
            WINDOW_EVENT_QUEUE_CAPACITY
                - 5
                - MAXIMUM_ACCEPTED_PRESSES * PRESS_END_RESERVATION as usize
        );
        // The forgotten presses can still end while there is room.
        drain(&mut queue);
        assert_eq!(
            queue.push(event(EventKind::Released, 1)),
            PushOutcome::Queued
        );
    }

    #[test]
    fn forgetting_a_target_forgets_its_press() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        queue.discard_target(object(1));
        assert!(!queue.follows_presses());

        queue.push(event(EventKind::Pressed, 2));
        queue.clear_target(object(2));
        assert!(!queue.follows_presses());

        queue.push(event(EventKind::Pressed, 3));
        queue.pop_front();
        queue.push(event(EventKind::Pressed, 4));
        queue.clear_dead_targets(|target| target == object(4));
        assert!(queue.follows_presses());
        assert_eq!(queue.followed_presses().len(), 1);
        assert_eq!(queue.followed_presses()[0].target, object(4));
    }

    #[test]
    fn losses_are_reported_at_most_once_per_interval() {
        let mut queue = EventQueue::new();
        assert_eq!(queue.take_loss_report(0), None);
        flood(&mut queue, EventKind::Key, 1);
        queue.push(event(EventKind::Key, 1));
        assert!(queue.has_unreported_losses());
        assert_eq!(
            queue.take_loss_report(5_000),
            Some(LossReport { lost: 2, total: 2 })
        );
        assert!(!queue.has_unreported_losses());

        // The consumer never drains the queue: losses keep being reported.
        for now in [5_100, 5_900] {
            queue.push(event(EventKind::Key, 1));
            assert_eq!(queue.take_loss_report(now), None);
        }
        queue.push(event(EventKind::Key, 1));
        assert_eq!(
            queue.take_loss_report(6_000),
            Some(LossReport { lost: 3, total: 5 })
        );
        assert_eq!(queue.take_loss_report(60_000), None, "nothing new lost");

        // Ticks wrap around.
        let mut queue = EventQueue::new();
        flood(&mut queue, EventKind::Key, 1);
        assert!(queue.take_loss_report(u32::MAX - 10).is_some());
        queue.push(event(EventKind::Key, 1));
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

    /// One input device: a pointer pressing targets 1 to 4, or a keypad
    /// pressing targets 5 and 6.
    struct Device {
        first_target: usize,
        targets: usize,
        press: Option<(usize, usize, bool)>,
    }

    /// Random LVGL-like streams with random consumer pauses: the consumer
    /// never sees the end of a press without its start, lifecycle events keep
    /// their order, and an accepted press always arrives whole.
    #[test]
    fn random_bursts_keep_presses_whole_and_in_order() {
        let mut random = Random(0x9E37_79B9_7F4A_7C15);
        let mut queue = EventQueue::new();
        let mut devices = [
            Device {
                first_target: 1,
                targets: 4,
                press: None,
            },
            Device {
                first_target: 5,
                targets: 2,
                press: None,
            },
        ];
        let mut sequence = 0;
        let mut next_press = 0;
        let mut delivered = Vec::new();
        // Per press: events that hold a reservation, pushed and delivered.
        let mut ends = Vec::new();

        let mut push = |queue: &mut EventQueue, code, target, press: usize| {
            sequence += 1;
            let mut queued = event(code, target);
            queued.pointer = Some([sequence, press as i32, 0, 0]);
            queue.push(queued);
        };

        for _ in 0..200_000 {
            match random.below(10) {
                0..=4 => {
                    let device = &mut devices[random.below(2)];
                    match device.press {
                        None => {
                            let target = device.first_target + random.below(device.targets);
                            next_press += 1;
                            ends.push((0, 0));
                            device.press = Some((target, next_press, false));
                            push(&mut queue, EventKind::Pressed, target, next_press);
                        }
                        Some((target, press, long)) => match random.below(12) {
                            0..=5 => push(&mut queue, EventKind::Pressing, target, press),
                            6 => push(&mut queue, EventKind::ValueChanged, target, press),
                            7 if !long => {
                                push(&mut queue, EventKind::LongPressed, target, press);
                                ends[press - 1].0 += 1;
                                device.press = Some((target, press, true));
                            }
                            7 => push(&mut queue, EventKind::LongPressedRepeat, target, press),
                            8 => {
                                push(&mut queue, EventKind::PressLost, target, press);
                                ends[press - 1].0 += 1;
                                device.press = None;
                            }
                            9 => {
                                // Released after a scroll: no click.
                                push(&mut queue, EventKind::Released, target, press);
                                ends[press - 1].0 += 1;
                                device.press = None;
                            }
                            _ => {
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
                                    push(&mut queue, code, target, press);
                                    ends[press - 1].0 += 1;
                                }
                                device.press = None;
                            }
                        },
                    }
                }
                5 => push(
                    &mut queue,
                    [EventKind::Key, EventKind::Focused, EventKind::Custom1][random.below(3)],
                    10 + random.below(3),
                    0,
                ),
                6 => push(
                    &mut queue,
                    [
                        EventKind::Scroll,
                        EventKind::ScrollEnd,
                        EventKind::StyleChanged,
                    ][random.below(3)],
                    1 + random.below(12),
                    0,
                ),
                7 => push(&mut queue, EventKind::ValueChanged, 20, 0),
                _ => {
                    if random.below(4) != 0 {
                        delivered.extend(queue.pop_front());
                    }
                }
            }
        }
        delivered.extend(core::iter::from_fn(|| queue.pop_front()));
        assert!(queue.dropped() > 10_000, "the stream must overflow");

        // Per target: the press being delivered and whether it was released.
        let mut state = [None::<(i32, bool)>; 21];
        let mut last_lifecycle = 0;
        let mut accepted = 0;
        for event in &delivered {
            let [sequence, press, _, _] = event.pointer.unwrap();
            let target = event.target as usize;
            if tier(event.code) == Tier::Lifecycle {
                assert!(sequence > last_lifecycle, "lifecycle events were reordered");
                last_lifecycle = sequence;
            }
            let Some(role) = press_role(event.code) else {
                continue;
            };
            if press == 0 {
                continue;
            }
            let current = state[target];
            match role {
                PressRole::Start => {
                    accepted += 1;
                    state[target] = Some((press, false));
                }
                PressRole::Progress | PressRole::LongPress | PressRole::Lost => {
                    assert_eq!(
                        current,
                        Some((press, false)),
                        "{:?} without its press",
                        event.code
                    );
                    if role == PressRole::Lost {
                        state[target] = None;
                    }
                }
                PressRole::Release => {
                    assert_eq!(current, Some((press, false)), "release without its press");
                    state[target] = Some((press, true));
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
            if role != PressRole::Start && role != PressRole::Progress {
                ends[press as usize - 1].1 += 1;
            }
        }
        assert!(accepted > 1_000, "presses must get through");
        for event in &delivered {
            let press = event.pointer.unwrap()[1] as usize;
            if event.code == EventKind::Pressed && press > 0 {
                let (pushed, received) = ends[press - 1];
                assert_eq!(pushed, received, "press {press} arrived incomplete");
            }
        }
    }

    #[test]
    fn key_information_is_captured_with_the_event() {
        let mut queue = EventQueue::new();
        queue.push(Event::new(EventKind::Key, object(1), Some(Key::Escape)));
        assert_eq!(queue.pop_front().unwrap().key, Some(Key::Escape));
    }

    #[test]
    fn discarding_a_target_preserves_the_order_of_the_others() {
        let mut queue = EventQueue::new();
        // Wrap the ring so the compaction crosses the end of the array.
        for _ in 0..12 {
            queue.push(event(EventKind::Clicked, 7));
        }
        for _ in 0..12 {
            queue.pop_front();
        }
        for (index, target) in [1, 2, 1, 3, 2, 1, 3, 3].into_iter().enumerate() {
            queue.push(event(
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
            queue.push(event(EventKind::Clicked, index + 1));
        }
        assert!(queue.is_full());
    }

    #[test]
    fn clearing_a_target_keeps_events_but_nulls_the_target() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        queue.push(event(EventKind::Clicked, 2));
        queue.push(event(EventKind::Released, 1));
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
            queue.push(event(EventKind::Clicked, target));
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
            queue.push(event(code, index % 3 + 1));
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
