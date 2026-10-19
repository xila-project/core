use crate::lvgl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum State {
    #[default]
    Released,
    Pressed,
}

impl From<State> for lvgl::lv_indev_state_t {
    fn from(value: State) -> lvgl::lv_indev_state_t {
        match value {
            State::Pressed => lvgl::lv_indev_state_t_LV_INDEV_STATE_PRESSED,
            State::Released => lvgl::lv_indev_state_t_LV_INDEV_STATE_RELEASED,
        }
    }
}

impl From<State> for u8 {
    fn from(value: State) -> u8 {
        value as u8
    }
}

impl TryFrom<u8> for State {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Released),
            1 => Ok(Self::Pressed),
            _ => Err(()),
        }
    }
}
