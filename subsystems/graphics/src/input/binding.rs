use file_system::DirectCharacterDevice;

use crate::lvgl;

use super::InputData;

/// This function is called by LVGL when it needs to read input data.
///
/// # Safety
///
/// This function may dereference a raw pointer.
pub unsafe extern "C" fn binding_callback_function(
    input_device: *mut lvgl::lv_indev_t,
    data: *mut lvgl::lv_indev_data_t,
) {
    let user_data = unsafe { lvgl::lv_indev_get_user_data(input_device) as *mut UserData };

    let device = unsafe { &(*user_data).device };

    let mut pointer_data = InputData::default();

    device
        .read_sync(pointer_data.as_mut(), 0)
        .expect("Error reading from input device");

    unsafe {
        *data = pointer_data.into();
    }
}

pub struct UserData {
    pub device: &'static dyn DirectCharacterDevice,
}
