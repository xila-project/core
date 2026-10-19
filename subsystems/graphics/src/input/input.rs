use alloc::boxed::Box;
use file_system::DirectCharacterDevice;

use core::ffi::c_void;

use crate::{Result, lvgl};

use super::{InputKind, UserData, binding_callback_function};

pub struct Input {
    #[allow(dead_code)]
    input_device: *mut lvgl::lv_indev_t,
}

impl Drop for Input {
    fn drop(&mut self) {
        unsafe {
            let _ = Box::from_raw(lvgl::lv_indev_get_user_data(self.input_device) as *mut UserData);

            lvgl::lv_indev_delete(self.input_device);

            // User_data will be dropped here.
        }
    }
}

unsafe impl Send for Input {}

unsafe impl Sync for Input {}

impl Input {
    pub fn new(device: &'static dyn DirectCharacterDevice, kind: InputKind) -> Result<Self> {
        // User_data is a pinned box, so it's ownership can be transferred to LVGL and will not move or dropper until the Input_device is dropped.
        let user_data = Box::new(UserData { device });

        let input_device = unsafe {
            let input_device = lvgl::lv_indev_create();
            lvgl::lv_indev_set_type(input_device, kind.into());
            lvgl::lv_indev_set_read_cb(input_device, Some(binding_callback_function));
            lvgl::lv_indev_set_user_data(input_device, Box::into_raw(user_data) as *mut c_void);

            if kind == InputKind::Keypad {
                let group = lvgl::lv_group_get_default();

                lvgl::lv_indev_set_group(input_device, group);
            }

            input_device
        };

        Ok(Self { input_device })
    }
}
