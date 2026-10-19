use alloc::collections::VecDeque;
use core::cell::{Cell, RefCell};

use synchronization::{
    Arc,
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
};

use crate::{Error, Point, Result};

use super::{InputData, InputKind, Key, State};

pub(crate) const INPUT_EVENT_QUEUE_CAPACITY: usize = 512;

#[derive(Clone, Copy)]
enum InjectedEvent {
    Pointer { point: Point, state: State },
    Key { key: Key, state: State },
}

pub(crate) struct InputQueue {
    events: Mutex<CriticalSectionRawMutex, RefCell<VecDeque<InjectedEvent>>>,
    pointer: Mutex<CriticalSectionRawMutex, Cell<(Point, State)>>,
}

impl InputQueue {
    pub(crate) fn new() -> Result<Arc<Self>> {
        let mut events = VecDeque::new();
        events
            .try_reserve_exact(INPUT_EVENT_QUEUE_CAPACITY)
            .map_err(|_| Error::OutOfMemory)?;
        Ok(Arc::new(Self {
            events: Mutex::new(RefCell::new(events)),
            pointer: Mutex::new(Cell::new((Point::default(), State::Released))),
        }))
    }

    fn enqueue(&self, event: InjectedEvent) -> Result<()> {
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            if events.len() >= INPUT_EVENT_QUEUE_CAPACITY {
                return Err(Error::AlreadyInUse);
            }
            events.push_back(event);
            Ok(())
        })
    }

    pub(crate) fn pointer_move(&self, point: Point) -> Result<()> {
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            if events.len() >= INPUT_EVENT_QUEUE_CAPACITY {
                return Err(Error::AlreadyInUse);
            }
            let state = self.pointer.lock(|last| {
                let (_, state) = last.get();
                last.set((point, state));
                state
            });
            events.push_back(InjectedEvent::Pointer { point, state });
            Ok(())
        })
    }

    pub(crate) fn pointer_button(&self, state: State) -> Result<()> {
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            if events.len() >= INPUT_EVENT_QUEUE_CAPACITY {
                return Err(Error::AlreadyInUse);
            }
            let (point, _) = self.pointer.lock(|last| {
                let (point, _) = last.get();
                last.set((point, state));
                (point, state)
            });
            events.push_back(InjectedEvent::Pointer { point, state });
            Ok(())
        })
    }

    pub(crate) fn key(&self, key: Key, state: State) -> Result<()> {
        self.enqueue(InjectedEvent::Key { key, state })
    }

    pub(crate) fn text(&self, text: &str) -> Result<usize> {
        if !text.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
            return Err(Error::InvalidParameter);
        }
        let event_count = text.len().checked_mul(2).ok_or(Error::InvalidParameter)?;
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            if events.len().saturating_add(event_count) > INPUT_EVENT_QUEUE_CAPACITY {
                return Err(Error::AlreadyInUse);
            }
            events
                .try_reserve(event_count)
                .map_err(|_| Error::OutOfMemory)?;
            for byte in text.bytes() {
                let key = Key::Character(byte);
                events.push_back(InjectedEvent::Key {
                    key,
                    state: State::Pressed,
                });
                events.push_back(InjectedEvent::Key {
                    key,
                    state: State::Released,
                });
            }
            Ok(text.len())
        })
    }

    pub(crate) fn pop_for(&self, kind: InputKind) -> Option<InputData> {
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            let matches = matches!(
                (kind, events.front()),
                (InputKind::Pointer, Some(InjectedEvent::Pointer { .. }))
                    | (InputKind::Keypad, Some(InjectedEvent::Key { .. }))
            );
            if !matches {
                return None;
            }
            match events.pop_front()? {
                InjectedEvent::Pointer { point, state } => {
                    Some(InputData::new(point, state, Key::Character(0), false))
                }
                InjectedEvent::Key { key, state } => {
                    Some(InputData::new(Point::default(), state, key, false))
                }
            }
        })
    }

    pub(crate) fn current_pointer(&self) -> InputData {
        self.pointer.lock(|pointer| {
            let (point, state) = pointer.get();
            InputData::new(point, state, Key::Character(0), false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injected_key_queue_preserves_press_release_order() {
        let queue = InputQueue::new().unwrap();
        queue.key(Key::Enter, State::Pressed).unwrap();
        queue.key(Key::Enter, State::Released).unwrap();

        let press = queue.pop_for(InputKind::Keypad).unwrap();
        let release = queue.pop_for(InputKind::Keypad).unwrap();
        assert_eq!(press.get_key(), Key::Enter);
        assert_eq!(*press.get_touch(), State::Pressed);
        assert_eq!(release.get_key(), Key::Enter);
        assert_eq!(*release.get_touch(), State::Released);
    }

    #[test]
    fn injected_text_expands_ascii_and_rejects_non_ascii() {
        let queue = InputQueue::new().unwrap();
        assert_eq!(queue.text("OK"), Ok(2));
        assert_eq!(queue.text("é"), Err(Error::InvalidParameter));
        for expected in [
            (b'O', State::Pressed),
            (b'O', State::Released),
            (b'K', State::Pressed),
            (b'K', State::Released),
        ] {
            let event = queue.pop_for(InputKind::Keypad).unwrap();
            assert_eq!(event.get_key(), Key::Character(expected.0));
            assert_eq!(*event.get_touch(), expected.1);
        }
        assert!(queue.pop_for(InputKind::Keypad).is_none());
    }

    #[test]
    fn injected_key_queue_returns_busy_at_capacity() {
        let queue = InputQueue::new().unwrap();
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY {
            queue.key(Key::Enter, State::Pressed).unwrap();
        }
        assert_eq!(
            queue.key(Key::Enter, State::Released),
            Err(Error::AlreadyInUse)
        );
    }
}
