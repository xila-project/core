use crate::ffi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Pointer,
    Keypad,
}

impl From<InputKind> for ffi::lv_indev_type_t {
    fn from(value: InputKind) -> Self {
        match value {
            InputKind::Pointer => ffi::lv_indev_type_t_LV_INDEV_TYPE_POINTER,
            InputKind::Keypad => ffi::lv_indev_type_t_LV_INDEV_TYPE_KEYPAD,
        }
    }
}
