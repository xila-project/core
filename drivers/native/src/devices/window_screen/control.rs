use graphics::{InputData, Key, Point, State};
use synchronization::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Sender, rwlock::RwLock,
};
use task::block_on;

use super::inner_window::InnerWindow;

#[cfg(unix)]
pub mod device;
pub mod protocol;
#[cfg(unix)]
pub mod transport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    DisplayUnavailable,
    InvalidCoordinates,
    UnsupportedKey,
    UnsupportedCharacter,
    QueueFull { queued_events: usize },
}

pub struct ControlOperations<'a> {
    inner_window: &'a InnerWindow,
    pointer: &'a RwLock<CriticalSectionRawMutex, InputData>,
    keyboard: Sender<'a, CriticalSectionRawMutex, (Key, State), 512>,
}

impl<'a> ControlOperations<'a> {
    pub fn new(
        inner_window: &'a InnerWindow,
        pointer: &'a RwLock<CriticalSectionRawMutex, InputData>,
        keyboard: Sender<'a, CriticalSectionRawMutex, (Key, State), 512>,
    ) -> Self {
        Self {
            inner_window,
            pointer,
            keyboard,
        }
    }

    pub fn resolution(&self) -> Result<Point, ControlError> {
        block_on(self.inner_window.get_resolution()).ok_or(ControlError::DisplayUnavailable)
    }

    pub fn pointer_move(&self, x: u32, y: u32) -> Result<(), ControlError> {
        let resolution = self.resolution()?;
        validate_coordinates(x, y, resolution)?;
        let mut pointer = self
            .pointer
            .try_write()
            .map_err(|_| ControlError::DisplayUnavailable)?;
        pointer.set_point(Point::new(x as i16, y as i16));
        Ok(())
    }

    pub fn pointer_button(&self, state: State) -> Result<(), ControlError> {
        let mut pointer = self
            .pointer
            .try_write()
            .map_err(|_| ControlError::DisplayUnavailable)?;
        pointer.set_state(state);
        Ok(())
    }

    pub fn key(&self, key: Key, state: State) -> Result<(), ControlError> {
        self.keyboard
            .try_send((key, state))
            .map_err(|_| ControlError::QueueFull { queued_events: 0 })
    }

    pub fn text(&self, text: &str) -> Result<usize, ControlError> {
        if !text.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
            return Err(ControlError::UnsupportedCharacter);
        }

        let mut queued = 0;
        for byte in text.bytes() {
            let key = Key::Character(byte);
            for state in [State::Pressed, State::Released] {
                if self.keyboard.try_send((key, state)).is_err() {
                    return Err(ControlError::QueueFull {
                        queued_events: queued,
                    });
                }
                queued += 1;
            }
        }
        Ok(queued)
    }
}

pub fn validate_coordinates(x: u32, y: u32, resolution: Point) -> Result<(), ControlError> {
    let width = u32::try_from(resolution.get_x()).map_err(|_| ControlError::InvalidCoordinates)?;
    let height = u32::try_from(resolution.get_y()).map_err(|_| ControlError::InvalidCoordinates)?;
    if x >= width || y >= height {
        return Err(ControlError::InvalidCoordinates);
    }
    Ok(())
}

pub fn parse_key(value: &str) -> Result<Key, ControlError> {
    let key = match value {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "escape" => Key::Escape,
        "delete" => Key::Delete,
        "backspace" => Key::Backspace,
        "enter" => Key::Enter,
        "next" => Key::Next,
        "previous" => Key::Previous,
        "home" => Key::Home,
        "end" => Key::End,
        value if value.len() == 1 && value.as_bytes()[0].is_ascii_graphic() => {
            Key::Character(value.as_bytes()[0])
        }
        _ => return Err(ControlError::UnsupportedKey),
    };
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use synchronization::channel::Channel;

    static KEYBOARD: Channel<CriticalSectionRawMutex, (Key, State), 512> = Channel::new();
    static KEYBOARD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static POINTER: RwLock<CriticalSectionRawMutex, InputData> =
        RwLock::new(InputData::default_constant());
    static WINDOW: InnerWindow = InnerWindow::new();

    #[test]
    fn coordinates_accept_inside_and_reject_right_bottom_edges() {
        let resolution = Point::new(800, 600);
        assert_eq!(validate_coordinates(799, 599, resolution), Ok(()));
        assert_eq!(
            validate_coordinates(800, 0, resolution),
            Err(ControlError::InvalidCoordinates)
        );
        assert_eq!(
            validate_coordinates(0, 600, resolution),
            Err(ControlError::InvalidCoordinates)
        );
    }

    #[test]
    fn key_mapping_rejects_unknown_or_multibyte_keys() {
        assert_eq!(parse_key("escape"), Ok(Key::Escape));
        assert_eq!(parse_key("A"), Ok(Key::Character(b'A')));
        assert_eq!(parse_key("🦊"), Err(ControlError::UnsupportedKey));
        assert_eq!(parse_key("not-a-key"), Err(ControlError::UnsupportedKey));
    }

    #[test]
    fn key_injection_preserves_pressed_and_released_state() {
        let _guard = KEYBOARD_TEST_LOCK.lock().unwrap();
        let controls = ControlOperations::new(&WINDOW, &POINTER, KEYBOARD.sender());
        controls.key(Key::Enter, State::Pressed).unwrap();
        controls.key(Key::Enter, State::Released).unwrap();
        let receiver = KEYBOARD.receiver();
        assert_eq!(receiver.try_receive(), Ok((Key::Enter, State::Pressed)));
        assert_eq!(receiver.try_receive(), Ok((Key::Enter, State::Released)));
    }

    #[test]
    fn text_reports_queue_full_without_claiming_success() {
        let _guard = KEYBOARD_TEST_LOCK.lock().unwrap();
        let controls = ControlOperations::new(&WINDOW, &POINTER, KEYBOARD.sender());
        for _ in 0..512 {
            KEYBOARD.try_send((Key::Enter, State::Pressed)).unwrap();
        }
        assert_eq!(
            controls.text("x"),
            Err(ControlError::QueueFull { queued_events: 0 })
        );
        for _ in 0..512 {
            let _ = KEYBOARD.receiver().try_receive();
        }
    }
}
