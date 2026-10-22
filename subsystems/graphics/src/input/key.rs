use crate::lvgl;

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
            lvgl::lv_key_t_LV_KEY_UP => Key::Up,
            lvgl::lv_key_t_LV_KEY_DOWN => Key::Down,
            lvgl::lv_key_t_LV_KEY_RIGHT => Key::Right,
            lvgl::lv_key_t_LV_KEY_LEFT => Key::Left,
            lvgl::lv_key_t_LV_KEY_ESC => Key::Escape,
            lvgl::lv_key_t_LV_KEY_DEL => Key::Delete,
            lvgl::lv_key_t_LV_KEY_BACKSPACE => Key::Backspace,
            lvgl::lv_key_t_LV_KEY_ENTER => Key::Enter,
            lvgl::lv_key_t_LV_KEY_NEXT => Key::Next,
            lvgl::lv_key_t_LV_KEY_PREV => Key::Previous,
            lvgl::lv_key_t_LV_KEY_HOME => Key::Home,
            lvgl::lv_key_t_LV_KEY_END => Key::End,
            character => Key::Character(character as u8),
        }
    }
}

impl From<Key> for u32 {
    fn from(value: Key) -> u32 {
        match value {
            Key::Up => lvgl::lv_key_t_LV_KEY_UP,
            Key::Down => lvgl::lv_key_t_LV_KEY_DOWN,
            Key::Right => lvgl::lv_key_t_LV_KEY_RIGHT,
            Key::Left => lvgl::lv_key_t_LV_KEY_LEFT,
            Key::Escape => lvgl::lv_key_t_LV_KEY_ESC,
            Key::Delete => lvgl::lv_key_t_LV_KEY_DEL,
            Key::Backspace => lvgl::lv_key_t_LV_KEY_BACKSPACE,
            Key::Enter => lvgl::lv_key_t_LV_KEY_ENTER,
            Key::Next => lvgl::lv_key_t_LV_KEY_NEXT,
            Key::Previous => lvgl::lv_key_t_LV_KEY_PREV,
            Key::Home => lvgl::lv_key_t_LV_KEY_HOME,
            Key::End => lvgl::lv_key_t_LV_KEY_END,
            Key::Character(character) => character as u32,
        }
    }
}
