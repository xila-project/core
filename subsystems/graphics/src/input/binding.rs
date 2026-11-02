use file_system::DirectCharacterDevice;
use synchronization::Arc;

use crate::lvgl;

use super::{InputData, InputKind, InputQueue};

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

    let user_data = unsafe { &*user_data };
    let input_data = user_data
        .injected
        .as_ref()
        .and_then(|queue| queue.pop_for(user_data.kind))
        .or_else(|| {
            (user_data.kind == InputKind::Pointer)
                .then(|| {
                    user_data
                        .injected
                        .as_ref()
                        .map(|queue| queue.current_pointer().into())
                })
                .flatten()
        })
        .or_else(|| {
            let mut data = InputData::default();
            if let Some(device) = user_data.device {
                device
                    .read_sync(data.as_mut(), 0)
                    .expect("Error reading from input device");
            }
            Some(data.into())
        })
        .unwrap_or_default();

    unsafe {
        *data = input_data;
    }
}

pub(super) struct UserData {
    pub(super) device: Option<&'static dyn DirectCharacterDevice>,
    pub(super) injected: Option<Arc<InputQueue>>,
    pub(super) kind: InputKind,
}
