use alloc::collections::VecDeque;
use core::cell::{Cell, RefCell};

use synchronization::{
    Arc,
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
};

use crate::{Error, Point, Result, lvgl};

use super::{InputData, InputKind, Key, State};

pub(crate) const INPUT_EVENT_QUEUE_CAPACITY: usize = 512;

#[derive(Clone, Copy)]
enum InjectedEvent {
    Pointer { point: Point, state: State },
    Key { key: [u8; 4], state: State },
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

    /// Queue a whole pointer click, or leave the queue unchanged.
    pub(crate) fn pointer_click(&self, point: Point) -> Result<()> {
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            if events.len() > INPUT_EVENT_QUEUE_CAPACITY - 3 {
                return Err(Error::AlreadyInUse);
            }
            self.pointer.lock(|pointer| {
                if pointer.get().1 != State::Released {
                    return Err(Error::AlreadyInUse);
                }
                for state in [State::Released, State::Pressed, State::Released] {
                    events.push_back(InjectedEvent::Pointer { point, state });
                }
                pointer.set((point, State::Released));
                Ok(())
            })
        })
    }

    /// Enqueue as many whole printable Unicode characters as fit, returning bytes accepted.
    pub(crate) fn text_chunk(&self, text: &str) -> Result<usize> {
        if text.chars().any(char::is_control) {
            return Err(Error::InvalidParameter);
        }
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            let capacity = (INPUT_EVENT_QUEUE_CAPACITY - events.len()) / 2;
            if capacity == 0 && !text.is_empty() {
                return Err(Error::AlreadyInUse);
            }
            let mut bytes = 0;
            for character in text.chars().take(capacity) {
                let mut encoded = [0; 4];
                let length = character.encode_utf8(&mut encoded).len();
                // LVGL treats text keys as native-endian packed UTF-8, while
                // ASCII keys remain their numeric byte value.
                let key = if length == 1 {
                    u32::from(encoded[0])
                } else {
                    u32::from_ne_bytes(encoded)
                };
                for state in [State::Pressed, State::Released] {
                    events.push_back(InjectedEvent::Key {
                        key: key.to_ne_bytes(),
                        state,
                    });
                }
                bytes += length;
            }
            Ok(bytes)
        })
    }

    pub(crate) fn key_press(&self, key: Key) -> Result<()> {
        self.events.lock(|events| {
            let mut events = events.borrow_mut();
            if events.len() > INPUT_EVENT_QUEUE_CAPACITY - 2 {
                return Err(Error::AlreadyInUse);
            }
            for state in [State::Pressed, State::Released] {
                events.push_back(InjectedEvent::Key {
                    key: u32::from(key).to_ne_bytes(),
                    state,
                });
            }
            Ok(())
        })
    }

    pub(crate) fn key(&self, key: Key, state: State) -> Result<()> {
        self.enqueue(InjectedEvent::Key {
            key: u32::from(key).to_ne_bytes(),
            state,
        })
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
                    key: u32::from(key).to_ne_bytes(),
                    state: State::Pressed,
                });
                events.push_back(InjectedEvent::Key {
                    key: u32::from(key).to_ne_bytes(),
                    state: State::Released,
                });
            }
            Ok(text.len())
        })
    }

    pub(crate) fn pop_for(&self, kind: InputKind) -> Option<lvgl::lv_indev_data_t> {
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
                    Some(InputData::new(point, state, Key::Character(0), false).into())
                }
                InjectedEvent::Key { key, state } => Some(lvgl::lv_indev_data_t {
                    key: u32::from_ne_bytes(key),
                    state: state.into(),
                    ..Default::default()
                }),
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
    fn click_is_atomic_at_capacity_and_preserves_order() {
        let queue = InputQueue::new().unwrap();
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY - 2 {
            queue.key(Key::Enter, State::Released).unwrap();
        }
        let point = Point::new(12, 17);
        assert_eq!(queue.pointer_click(point), Err(Error::AlreadyInUse));
        assert_eq!(queue.current_pointer().get_point(), &Point::default());
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY - 2 {
            queue.pop_for(InputKind::Keypad).unwrap();
        }
        queue.pointer_click(point).unwrap();
        for state in [State::Released, State::Pressed, State::Released] {
            let event = queue.pop_for(InputKind::Pointer).unwrap();
            assert_eq!(event.point.x, 12);
            assert_eq!(event.point.y, 17);
            assert_eq!(event.state, state.into());
        }
    }

    #[test]
    fn unicode_text_queues_whole_characters_with_backpressure() {
        let queue = InputQueue::new().unwrap();
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY - 2 {
            queue.key(Key::Enter, State::Released).unwrap();
        }
        assert_eq!(queue.text_chunk("é中"), Ok(2));
        assert_eq!(queue.text_chunk("中"), Err(Error::AlreadyInUse));
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY - 2 {
            queue.pop_for(InputKind::Keypad).unwrap();
        }
        let press = queue.pop_for(InputKind::Keypad).unwrap();
        let release = queue.pop_for(InputKind::Keypad).unwrap();
        assert_eq!(press.key, u32::from_ne_bytes([0xc3, 0xa9, 0, 0]));
        assert_eq!(press.state, State::Pressed.into());
        assert_eq!(release.key, press.key);
        assert_eq!(release.state, State::Released.into());
        assert_eq!(queue.text_chunk("中"), Ok(3));
        assert_eq!(
            queue.pop_for(InputKind::Keypad).unwrap().key,
            u32::from_ne_bytes([0xe4, 0xb8, 0xad, 0])
        );
    }

    #[test]
    fn long_text_can_be_retried_without_partial_characters() {
        let queue = InputQueue::new().unwrap();
        let text = "中".repeat(400);
        let mut offset = 0;
        let mut characters = 0;
        while offset < text.len() {
            let accepted = queue.text_chunk(&text[offset..]).unwrap();
            assert!(accepted > 0 && text.is_char_boundary(offset + accepted));
            offset += accepted;
            while let Some(press) = queue.pop_for(InputKind::Keypad) {
                let release = queue.pop_for(InputKind::Keypad).unwrap();
                assert_eq!(press.state, State::Pressed.into());
                assert_eq!(release.state, State::Released.into());
                assert_eq!(press.key, release.key);
                characters += 1;
            }
        }
        assert_eq!(characters, 400);
    }

    #[test]
    fn unsupported_controls_do_not_enqueue_a_prefix() {
        let queue = InputQueue::new().unwrap();
        assert_eq!(queue.text_chunk("a\nb"), Err(Error::InvalidParameter));
        assert!(queue.pop_for(InputKind::Keypad).is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unicode_key_events_insert_into_lvgl_textarea() {
        // LVGL owns process-global state; isolate it from other parallel unit tests.
        extern crate std;
        if std::env::var_os("XILA_INPUT_LVGL_TEST_CHILD").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "input::injection::tests::unicode_key_events_insert_into_lvgl_textarea",
                ])
                .env("XILA_INPUT_LVGL_TEST_CHILD", "1")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        unsafe {
            lvgl::lv_init();
            let display = lvgl::lv_display_create(32, 32);
            assert!(!display.is_null());
            let screen = lvgl::lv_display_get_screen_active(display);
            let textarea = lvgl::lv_textarea_create(screen);
            assert!(!textarea.is_null());
            let queue = InputQueue::new().unwrap();
            assert_eq!(queue.text_chunk("Aé中"), Ok("Aé中".len()));
            for _ in 0..3 {
                let mut press = queue.pop_for(InputKind::Keypad).unwrap();
                let release = queue.pop_for(InputKind::Keypad).unwrap();
                assert_eq!(press.key, release.key);
                lvgl::lv_obj_send_event(
                    textarea,
                    lvgl::lv_event_code_t_LV_EVENT_KEY,
                    (&mut press.key as *mut u32).cast(),
                );
            }
            let value = core::ffi::CStr::from_ptr(lvgl::lv_textarea_get_text(textarea));
            assert_eq!(value.to_str().unwrap(), "Aé中");
            lvgl::lv_display_delete(display);
            lvgl::lv_deinit();
        }
    }

    #[test]
    fn key_press_is_not_partially_queued() {
        let queue = InputQueue::new().unwrap();
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY - 1 {
            queue.key(Key::Enter, State::Released).unwrap();
        }
        assert_eq!(queue.key_press(Key::Enter), Err(Error::AlreadyInUse));
        for _ in 0..INPUT_EVENT_QUEUE_CAPACITY - 1 {
            queue.pop_for(InputKind::Keypad).unwrap();
        }
        assert!(queue.pop_for(InputKind::Keypad).is_none());
        queue.key_press(Key::Enter).unwrap();
        assert_eq!(
            queue.pop_for(InputKind::Keypad).unwrap().state,
            State::Pressed.into()
        );
        assert_eq!(
            queue.pop_for(InputKind::Keypad).unwrap().state,
            State::Released.into()
        );
    }

    #[test]
    fn injected_key_queue_preserves_press_release_order() {
        let queue = InputQueue::new().unwrap();
        queue.key(Key::Enter, State::Pressed).unwrap();
        queue.key(Key::Enter, State::Released).unwrap();

        let press = queue.pop_for(InputKind::Keypad).unwrap();
        let release = queue.pop_for(InputKind::Keypad).unwrap();
        assert_eq!(press.key, u32::from(Key::Enter));
        assert_eq!(press.state, State::Pressed.into());
        assert_eq!(release.key, u32::from(Key::Enter));
        assert_eq!(release.state, State::Released.into());
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
            assert_eq!(event.key, u32::from(Key::Character(expected.0)));
            assert_eq!(event.state, expected.1.into());
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
