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
    /// The ring was full: the oldest coalescible event was dropped to make room
    /// and the event was appended.
    EvictedOldest,
    /// The ring was full of events that must be kept: the event was dropped.
    Dropped,
}

/// Bounded FIFO of window events.
pub struct EventQueue {
    slots: [Option<Event>; WINDOW_EVENT_QUEUE_CAPACITY],
    head: u8,
    length: u8,
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

impl EventQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; WINDOW_EVENT_QUEUE_CAPACITY],
            head: 0,
            length: 0,
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

    /// Number of events lost to overflow since creation, both rejected events
    /// and coalescible events evicted to make room.
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Append an event, keeping the ring bounded.
    ///
    /// 1. A pointer move (`Pressing`) directly following a pointer move for the
    ///    same target replaces it: only the latest position is useful.
    /// 2. Otherwise, if the ring is full, the oldest queued pointer move is
    ///    dropped to make room, so key, click, press and release events are
    ///    never lost while a move can be sacrificed.
    /// 3. If the ring is still full, the new event is dropped (keeping the
    ///    queue order and front stable) and counted in [`Self::dropped`].
    pub fn push(&mut self, event: Event) -> PushOutcome {
        if is_coalescible(&event)
            && let Some(last) = self.last_mut()
            && is_coalescible(last)
            && last.target == event.target
        {
            *last = event;
            return PushOutcome::Coalesced;
        }

        let mut outcome = PushOutcome::Queued;
        if self.is_full() {
            let Some(index) =
                (0..self.len()).find(|&index| self.get(index).is_some_and(is_coalescible))
            else {
                self.dropped = self.dropped.saturating_add(1);
                return PushOutcome::Dropped;
            };
            self.remove(index);
            self.dropped = self.dropped.saturating_add(1);
            outcome = PushOutcome::EvictedOldest;
        }

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
    fn full_ring_evicts_the_oldest_pointer_move_and_keeps_everything_else() {
        let mut queue = EventQueue::new();
        queue.push(event(EventKind::Pressed, 1));
        queue.push(pointer_move(1, 1)); // oldest move: evicted first
        queue.push(event(EventKind::Key, 1));
        queue.push(pointer_move(2, 2));
        queue.push(event(EventKind::Key, 2)); // so the next move is not coalesced
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
        assert_eq!(drained[1], (EventKind::Key, 1));
        assert_eq!(drained[2], (EventKind::Pressing, 2));
        assert_eq!(*drained.last().unwrap(), (EventKind::Released, 1));
        assert_eq!(
            drained
                .iter()
                .filter(|(code, _)| *code == EventKind::Pressing)
                .count(),
            1
        );
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
    fn full_ring_of_important_events_drops_the_new_one_and_counts_it() {
        let mut queue = EventQueue::new();
        for index in 0..WINDOW_EVENT_QUEUE_CAPACITY {
            queue.push(event(EventKind::Clicked, index + 1));
        }
        for _ in 0..3 {
            assert_eq!(queue.push(event(EventKind::Key, 99)), PushOutcome::Dropped);
        }
        assert_eq!(queue.push(pointer_move(1, 0)), PushOutcome::Dropped);
        assert_eq!(queue.dropped(), 4);
        assert_eq!(queue.len(), WINDOW_EVENT_QUEUE_CAPACITY);
        let drained = drain(&mut queue);
        assert!(drained.iter().all(|(code, _)| *code == EventKind::Clicked));
        assert_eq!(drained[0].1, 1);
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
