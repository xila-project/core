use crate::error::Result;
use xila::graphics::{Event, lvgl};
use xila::internationalization::translate;

pub struct GeneralTab {
    tab_container: *mut lvgl::lv_obj_t,
}

impl GeneralTab {
    pub fn new() -> Self {
        Self {
            tab_container: core::ptr::null_mut(),
        }
    }

    pub async fn create_ui(
        &mut self,
        parent_tabview: *mut lvgl::lv_obj_t,
    ) -> Result<*mut lvgl::lv_obj_t> {
        let tab_container = unsafe {
            lvgl::lv_tabview_add_tab(parent_tabview, translate!(c"tabs.general").as_ptr())
        };

        if tab_container.is_null() {
            return Err(crate::error::Error::FailedToCreateUiElement);
        }

        self.tab_container = tab_container;

        // Create general settings UI here
        unsafe {
            let info_label = lvgl::lv_label_create(tab_container);
            lvgl::lv_label_set_text(
                info_label,
                translate!(c"general.status.coming_soon").as_ptr(),
            );
            lvgl::lv_obj_align(info_label, lvgl::lv_align_t_LV_ALIGN_CENTER, 0, 0);
        }

        Ok(tab_container)
    }

    pub async fn handle_event(&mut self, _event: &Event) -> bool {
        // For now, the general tab doesn't handle any specific events
        false
    }
}
