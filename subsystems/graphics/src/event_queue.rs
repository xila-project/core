//! Fixed-capacity event ring owned by each window.
//!
//! The ring lives inline in the window object, so it never allocates, never
//! grows and never fragments a heap, however many events a burst produces.
//! Overflow is resolved by an explicit policy (see [`EventQueue::push`]).

use crate::{Event, EventKind};
use core::ptr;

/// Number of events a window can hold before its overflow policy applies.
pub const WINDOW_EVENT_QUEUE_CAPACITY: usize = 16;

/// What [`EventQueue::push`] did with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// The event was appended.
    Queued,
    /// The event replaced the queued pointer move it superseded.
    Coalesced,
    /// The event replaced the oldest queued event eligible for eviction.
    EvictedOldest,
    /// The event replaced the oldest high-priority event and the window should
    /// warn about this burst of input loss.
    FirstInputEvictionInBurst,
    /// The ring was full of high-priority events and rejected a low-priority arrival.
    Dropped,
}

/// Bounded queue of window events in arrival order.
///
/// Consecutive pointer moves for the same target coalesce. When full, the queue
/// evicts its oldest low-priority event first. Scroll steps, hover, style and
/// drawing notifications are low priority. If all queued events are high
/// priority, a high-priority arrival replaces the oldest and a low-priority
/// arrival is rejected.
///
/// Each overflow loss is counted. The first high-priority eviction requests a
/// warning, and another warning can be requested once the queue empties.
pub struct EventQueue {
    slots: [Option<Event>; WINDOW_EVENT_QUEUE_CAPACITY],
    head: u8,
    length: u8,
    input_eviction_in_burst: bool,
    dropped: u32,
}

// Indices are stored as `u8`.
const _: () =
    assert!(WINDOW_EVENT_QUEUE_CAPACITY > 0 && WINDOW_EVENT_QUEUE_CAPACITY <= u8::MAX as usize);

/// Whether a later event of the same kind and target supersedes this one:
/// pointer moves only carry the latest position.
const fn is_coalescible(event: &Event) -> bool {
    matches!(event.code, EventKind::Pressing)
}

/// Notifications that can be sacrificed to preserve input and state changes.
const fn is_low_priority(event: &Event) -> bool {
    matches!(
        event.code,
        EventKind::Scroll
            | EventKind::StyleChanged
            | EventKind::HoverOver
            | EventKind::HoverLeave
            | EventKind::CoverCheck
            | EventKind::RefreshExtDrawSize
            | EventKind::DrawMainBegin
            | EventKind::DrawMain
            | EventKind::DrawMainEnd
            | EventKind::DrawPostBegin
            | EventKind::DrawPost
            | EventKind::DrawPostEnd
            | EventKind::DrawTaskAdded
            | EventKind::GetSelfSize
            | EventKind::LayoutChanged
    )
}

impl EventQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; WINDOW_EVENT_QUEUE_CAPACITY],
            head: 0,
            length: 0,
            input_eviction_in_burst: false,
            dropped: 0,
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

    /// Number of events lost to overflow since creation, including rejections
    /// and evictions.
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Append an event according to the queue's eviction policy.
    pub fn push(&mut self, event: Event) -> PushOutcome {
        if is_coalescible(&event)
            && let Some(last) = self.last_mut()
            && is_coalescible(last)
            && last.target == event.target
        {
            *last = event;
            return PushOutcome::Coalesced;
        }

        let outcome = if self.is_full() {
            self.dropped = self.dropped.saturating_add(1);
            match (0..self.len()).find(|&index| self.get(index).is_some_and(is_low_priority)) {
                Some(index) => {
                    self.remove(index);
                    PushOutcome::EvictedOldest
                }
                None if is_low_priority(&event) => return PushOutcome::Dropped,
                None => {
                    self.pop_front();
                    if core::mem::replace(&mut self.input_eviction_in_burst, true) {
                        PushOutcome::EvictedOldest
                    } else {
                        PushOutcome::FirstInputEvictionInBurst
                    }
                }
            }
        } else {
            PushOutcome::Queued
        };

        let slot = self.physical(self.len());
        self.slots[slot] = Some(event);
        self.length += 1;
        outcome
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
        if self.is_empty() {
            self.input_eviction_in_burst = false;
        }
        event
    }

    /// Remove every event aimed at `target`, preserving the order of the rest.
    /// Only compares addresses; never dereferences `target`.
    pub fn discard_target(&mut self, target: *mut crate::lvgl::lv_obj_t) {
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
        if self.is_empty() {
            self.input_eviction_in_burst = false;
        }
    }

    /// Keep every event but null the ones aimed at `target`.
    /// Only compares addresses; never dereferences `target`.
    pub fn clear_target(&mut self, target: *mut crate::lvgl::lv_obj_t) {
        for event in self.slots.iter_mut().flatten() {
            if event.target == target {
                event.target = ptr::null_mut();
            }
        }
    }

    /// Null the target of every event whose target `is_live` rejects, and keep
    /// the events. `is_live` is called once per distinct run of targets and
    /// must only compare the address; the targets may already be deleted.
    pub fn clear_dead_targets(
        &mut self,
        mut is_live: impl FnMut(*mut crate::lvgl::lv_obj_t) -> bool,
    ) {
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

    fn last_mut(&mut self) -> Option<&mut Event> {
        let index = self.len().checked_sub(1)?;
        let slot = self.physical(index);
        self.slots[slot].as_mut()
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
    use crate::{Key, lvgl};
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
    fn consecutive_pointer_moves_for_one_target_keep_the_latest() {
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
    fn pointer_moves_for_other_targets_or_after_other_events_are_not_coalesced() {
        let mut queue = EventQueue::new();
        queue.push(pointer_move(1, 1));
        assert_eq!(queue.push(pointer_move(2, 2)), PushOutcome::Queued);
        queue.push(event(EventKind::Key, 2));
        assert_eq!(queue.push(pointer_move(2, 3)), PushOutcome::Queued);
        assert_eq!(queue.len(), 4);
    }

    #[test]
    fn non_pointer_events_never_coalesce() {
        let mut queue = EventQueue::new();
        for _ in 0..4 {
            queue.push(event(EventKind::ValueChanged, 1));
        }
        assert_eq!(queue.len(), 4);
    }

    #[test]
    fn full_ring_evicts_the_oldest_scroll_and_keeps_pointer_moves() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        queue.push(pointer_move(1, 1));
        queue.push(event(EventKind::Key, 1));
        queue.push(event(EventKind::Scroll, 2));
        queue.push(event(EventKind::StyleChanged, 3));
        while !queue.is_full() {
            queue.push(event(EventKind::Clicked, 3));
        }
        assert_eq!(
            queue.push(event(EventKind::Released, 1)),
            PushOutcome::EvictedOldest
        );
        assert!(queue.is_full());
        assert_eq!(queue.dropped(), 1);

        let drained = drain(&mut queue);
        assert_eq!(drained.len(), WINDOW_EVENT_QUEUE_CAPACITY);
        assert_eq!(drained[0], (EventKind::Pressed, 1));
        assert_eq!(drained[1], (EventKind::Pressing, 1));
        assert_eq!(drained[2], (EventKind::Key, 1));
        assert_eq!(drained[3], (EventKind::StyleChanged, 3));
        assert_eq!(*drained.last().unwrap(), (EventKind::Released, 1));
    }

    #[test]
    fn full_ring_coalesces_the_tail_before_evicting_an_older_move() {
        let mut queue = EventQueue::new();
        queue.push(pointer_move(1, 1));
        for _ in 0..WINDOW_EVENT_QUEUE_CAPACITY - 2 {
            queue.push(event(EventKind::Clicked, 1));
        }
        queue.push(pointer_move(2, 2));
        assert_eq!(queue.push(pointer_move(2, 3)), PushOutcome::Coalesced);
        assert!(queue.is_full());
        assert_eq!(queue.dropped(), 0);
        assert_eq!(queue.pop_front().unwrap().pointer, Some([1, 0, 100, 100]));
        for _ in 0..WINDOW_EVENT_QUEUE_CAPACITY - 2 {
            assert_eq!(queue.pop_front().unwrap().code, EventKind::Clicked);
        }
        assert_eq!(queue.pop_front().unwrap().pointer, Some([3, 0, 100, 100]));
    }

    #[test]
    fn full_ring_sacrifices_scroll_and_style_events_for_input_and_value_events() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        while !queue.is_full() {
            queue.push(event(EventKind::StyleChanged, 1));
            queue.push(event(EventKind::Scroll, 2));
        }
        let important = [
            EventKind::Released,
            EventKind::ShortClicked,
            EventKind::Clicked,
            EventKind::ValueChanged,
        ];
        for code in important {
            assert_eq!(queue.push(event(code, 1)), PushOutcome::EvictedOldest);
        }
        let drained = drain(&mut queue);
        assert_eq!(drained.len(), WINDOW_EVENT_QUEUE_CAPACITY);
        assert_eq!(drained[0], (EventKind::Pressed, 1));
        let codes: Vec<_> = drained.iter().map(|(code, _)| *code).collect();
        assert_eq!(codes[codes.len() - important.len()..], important);
    }

    #[test]
    fn full_ring_of_scroll_events_keeps_the_next_press() {
        let mut queue = EventQueue::new();
        for address in 1..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.push(event(EventKind::Scroll, address));
        }
        assert_eq!(
            queue.push(event(EventKind::Pressed, 99)),
            PushOutcome::EvictedOldest
        );
        let drained = drain(&mut queue);
        assert_eq!(drained.len(), WINDOW_EVENT_QUEUE_CAPACITY);
        assert_eq!(drained[0], (EventKind::Scroll, 2));
        assert_eq!(*drained.last().unwrap(), (EventKind::Pressed, 99));
    }

    #[test]
    fn full_ring_of_input_events_evicts_the_oldest_for_new_input() {
        let mut queue = EventQueue::new();
        for address in 1..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.push(event(EventKind::Clicked, address));
        }
        assert_eq!(
            queue.push(event(EventKind::Released, 99)),
            PushOutcome::FirstInputEvictionInBurst
        );
        let drained = drain(&mut queue);
        assert_eq!(drained.len(), WINDOW_EVENT_QUEUE_CAPACITY);
        assert_eq!(
            drained[..drained.len() - 1],
            (2..=WINDOW_EVENT_QUEUE_CAPACITY)
                .map(|address| (EventKind::Clicked, address))
                .collect::<Vec<_>>()
        );
        assert_eq!(*drained.last().unwrap(), (EventKind::Released, 99));
        assert_eq!(queue.dropped(), 1);
    }

    #[test]
    fn full_ring_of_input_events_rejects_low_priority_events() {
        let mut queue = EventQueue::new();
        for address in 1..=WINDOW_EVENT_QUEUE_CAPACITY {
            queue.push(pointer_move(address, 0));
        }
        let noise = [
            EventKind::Scroll,
            EventKind::StyleChanged,
            EventKind::DrawMain,
            EventKind::HoverOver,
            EventKind::HoverLeave,
        ];
        for code in noise {
            assert_eq!(queue.push(event(code, 99)), PushOutcome::Dropped);
        }
        assert_eq!(queue.dropped() as usize, noise.len());
        assert_eq!(
            drain(&mut queue),
            (1..=WINDOW_EVENT_QUEUE_CAPACITY)
                .map(|address| (EventKind::Pressing, address))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn input_evictions_request_one_warning_per_burst() {
        let mut queue = EventQueue::new();
        for clear_by_discard in [false, true, false] {
            for _ in 0..WINDOW_EVENT_QUEUE_CAPACITY {
                queue.push(event(EventKind::Clicked, 1));
            }
            assert_eq!(
                queue.push(event(EventKind::Scroll, 1)),
                PushOutcome::Dropped
            );

            let mut warning_count = 0;
            for _ in 0..10 {
                let outcome = queue.push(event(EventKind::Released, 1));
                warning_count += usize::from(outcome == PushOutcome::FirstInputEvictionInBurst);
                queue.pop_front();
                assert_eq!(
                    queue.push(event(EventKind::Clicked, 1)),
                    PushOutcome::Queued
                );
            }
            assert_eq!(warning_count, 1);

            if clear_by_discard {
                queue.discard_target(object(1));
            } else {
                while queue.pop_front().is_some() {}
            }
            assert!(queue.is_empty());
        }
        assert_eq!(queue.dropped(), 33);
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
                    EventKind::Pressed
                } else {
                    EventKind::Released
                },
                target,
            ));
        }
        queue.discard_target(object(1));
        assert_eq!(
            drain(&mut queue),
            [
                (EventKind::Released, 2),
                (EventKind::Released, 3),
                (EventKind::Pressed, 2),
                (EventKind::Pressed, 3),
                (EventKind::Released, 3),
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
            "Event={} Option<Event>={} old queue={}+{} new queue={} Window={}",
            size_of::<Event>(),
            size_of::<Option<Event>>(),
            size_of::<VecDeque<Event>>(),
            10 * size_of::<Event>(),
            size_of::<EventQueue>(),
            size_of::<crate::Window>()
        );
        assert!(size_of::<crate::Window>() < 65536);
        assert!(size_of::<EventQueue>() <= WINDOW_EVENT_QUEUE_CAPACITY * size_of::<Event>() + 8);
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
            if index % 7 == 0 {
                queue.pop_front();
            }
        }
        assert_eq!(size_of::<EventQueue>(), size);
        assert!(queue.dropped() > 0);
    }
}
