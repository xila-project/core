use crate::lvgl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Pointer,
    Keypad,
}

impl From<InputKind> for lvgl::lv_indev_type_t {
    fn from(value: InputKind) -> Self {
        match value {
            InputKind::Pointer => lvgl::lv_indev_type_t_LV_INDEV_TYPE_POINTER,
            InputKind::Keypad => lvgl::lv_indev_type_t_LV_INDEV_TYPE_KEYPAD,
        }
    }
}
