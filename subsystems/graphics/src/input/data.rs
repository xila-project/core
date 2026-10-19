use core::mem::transmute;

use crate::{Point, lvgl};

use super::{Key, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct InputData {
    pub r#continue: bool,
    pub point: Point,
    pub state: State,
    pub key: Key,
}

impl Default for InputData {
    fn default() -> Self {
        Self {
            point: Point::default(),
            state: State::default(),
            key: Key::Character(0),
            r#continue: false,
        }
    }
}

impl InputData {
    pub const fn new(point: Point, state: State, key: Key, r#continue: bool) -> Self {
        Self {
            point,
            state,
            key,
            r#continue,
        }
    }

    pub const fn default_constant() -> Self {
        Self {
            point: Point::new(0, 0),
            state: State::Released,
            key: Key::Character(0),
            r#continue: false,
        }
    }

    pub const fn get_continue(&self) -> bool {
        self.r#continue
    }

    pub const fn get_point(&self) -> &Point {
        &self.point
    }

    pub const fn get_touch(&self) -> &State {
        &self.state
    }

    pub const fn get_key(&self) -> Key {
        self.key
    }

    pub const fn set_continue(&mut self, r#continue: bool) {
        self.r#continue = r#continue;
    }

    pub fn set_point(&mut self, point: Point) {
        self.point = point;
    }

    pub fn set_state(&mut self, touch: State) {
        self.state = touch;
    }

    pub fn set_key(&mut self, key: Key) {
        self.key = key;
    }

    pub fn set(&mut self, point: Point, touch: State) {
        self.point = point;
        self.state = touch;
    }
}

impl TryFrom<&mut [u8]> for &mut InputData {
    type Error = ();

    fn try_from(value: &mut [u8]) -> Result<Self, Self::Error> {
        if value.len() != size_of::<InputData>() {
            return Err(());
        }
        if !(value.as_ptr() as usize).is_multiple_of(core::mem::align_of::<InputData>()) {
            return Err(());
        }

        #[allow(clippy::transmute_ptr_to_ref)]
        Ok(unsafe { transmute::<*mut u8, Self>(value.as_mut_ptr()) })
    }
}

impl AsMut<[u8]> for InputData {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut _ as *mut u8, size_of::<Self>()) }
    }
}

impl From<InputData> for lvgl::lv_indev_data_t {
    fn from(value: InputData) -> lvgl::lv_indev_data_t {
        let mut input_device_data = lvgl::lv_indev_data_t::default();

        let state = value.get_touch();

        if *state == State::Pressed {
            input_device_data.point = (*value.get_point()).into();
        }

        input_device_data.state = (*state).into();
        input_device_data.key = value.key.into();

        input_device_data
    }
}
