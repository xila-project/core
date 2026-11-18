use crate::ffi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Right,
    Left,
    Escape,
    Delete,
    Backspace,
    Enter,
    Next,
    Previous,
    Home,
    End,
    Character(u8),
}

impl From<u32> for Key {
    fn from(value: u32) -> Key {
        match value {
            ffi::lv_key_t_LV_KEY_UP => Key::Up,
            ffi::lv_key_t_LV_KEY_DOWN => Key::Down,
            ffi::lv_key_t_LV_KEY_RIGHT => Key::Right,
            ffi::lv_key_t_LV_KEY_LEFT => Key::Left,
            ffi::lv_key_t_LV_KEY_ESC => Key::Escape,
            ffi::lv_key_t_LV_KEY_DEL => Key::Delete,
            ffi::lv_key_t_LV_KEY_BACKSPACE => Key::Backspace,
            ffi::lv_key_t_LV_KEY_ENTER => Key::Enter,
            ffi::lv_key_t_LV_KEY_NEXT => Key::Next,
            ffi::lv_key_t_LV_KEY_PREV => Key::Previous,
            ffi::lv_key_t_LV_KEY_HOME => Key::Home,
            ffi::lv_key_t_LV_KEY_END => Key::End,
            character => Key::Character(character as u8),
        }
    }
}

impl From<Key> for u32 {
    fn from(value: Key) -> u32 {
        match value {
            Key::Up => ffi::lv_key_t_LV_KEY_UP,
            Key::Down => ffi::lv_key_t_LV_KEY_DOWN,
            Key::Right => ffi::lv_key_t_LV_KEY_RIGHT,
            Key::Left => ffi::lv_key_t_LV_KEY_LEFT,
            Key::Escape => ffi::lv_key_t_LV_KEY_ESC,
            Key::Delete => ffi::lv_key_t_LV_KEY_DEL,
            Key::Backspace => ffi::lv_key_t_LV_KEY_BACKSPACE,
            Key::Enter => ffi::lv_key_t_LV_KEY_ENTER,
            Key::Next => ffi::lv_key_t_LV_KEY_NEXT,
            Key::Previous => ffi::lv_key_t_LV_KEY_PREV,
            Key::Home => ffi::lv_key_t_LV_KEY_HOME,
            Key::End => ffi::lv_key_t_LV_KEY_END,
            Key::Character(character) => character as u32,
        }
    }
}
