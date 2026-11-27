use alloc::boxed::Box;
use file_system::DirectCharacterDevice;
use synchronization::Arc;

use core::cell::Cell;
use core::ffi::c_void;

use crate::{Result, lvgl};

use super::{InputKind, InputQueue, UserData, binding_callback_function};

pub struct Input {
    #[allow(dead_code)]
    input_device: *mut lvgl::lv_indev_t,
}

impl Drop for Input {
    fn drop(&mut self) {
        // SAFETY: construction requires the owner to drop this live input
        // under the graphics lock, before LVGL deinitializes.
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
    /// Register a physical input device.
    ///
    /// # Safety
    ///
    /// LVGL must be initialized. Hold its lock during registration and
    /// while dropping the returned input. Drop it before LVGL deinitializes.
    pub unsafe fn new(device: &'static dyn DirectCharacterDevice, kind: InputKind) -> Result<Self> {
        // SAFETY: the caller supplies the initialization and lock guarantees.
        unsafe { Self::create(Some(device), None, kind) }
    }

    pub(crate) unsafe fn new_virtual(queue: Arc<InputQueue>, kind: InputKind) -> Result<Self> {
        // SAFETY: the caller supplies the initialization and lock guarantees.
        unsafe { Self::create(None, Some(queue), kind) }
    }

    unsafe fn create(
        device: Option<&'static dyn DirectCharacterDevice>,
        injected: Option<Arc<InputQueue>>,
        kind: InputKind,
    ) -> Result<Self> {
        // User_data stays allocated until LVGL drops this input device.
        let user_data = Box::new(UserData {
            device,
            injected,
            events_read: Cell::new(0),
            kind,
        });

        // SAFETY: LVGL is initialized and the caller holds the graphics lock.
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
