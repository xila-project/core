use crate::error::Result;
use xila::graphics::{self, Event, lvgl};
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
        let tab_container =
            unsafe { lvgl::lv_tabview_add_tab(parent_tabview, translate!(c"General").as_ptr()) };

        if tab_container.is_null() {
            return Err(crate::error::Error::FailedToCreateUiElement);
        }

        self.tab_container = tab_container;

        // Keep the overview useful while this tab has no general controls yet.
        unsafe {
            lvgl::lv_obj_set_flex_flow(
                tab_container,
                lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN,
            );
            lvgl::lv_obj_set_style_pad_all(tab_container, 18, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_pad_row(tab_container, 12, lvgl::LV_STATE_DEFAULT);

            let card = lvgl::lv_obj_create(tab_container);
            if card.is_null() {
                return Err(crate::error::Error::FailedToCreateUiElement);
            }
            lvgl::lv_obj_set_width(card, lvgl::lv_pct(100));
            lvgl::lv_obj_set_height(card, lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_flow(card, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_style_pad_all(card, 18, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_pad_row(card, 8, lvgl::LV_STATE_DEFAULT);
            graphics::theme::set_surface(card, graphics::theme::Surface::Card);

            let title = lvgl::lv_label_create(card);
            lvgl::lv_label_set_text(title, translate!(c"General").as_ptr());

            let info_label = lvgl::lv_label_create(card);
            lvgl::lv_label_set_text(
                info_label,
                translate!(c"General settings will be implemented here.").as_ptr(),
            );
            lvgl::lv_label_set_long_mode(
                info_label,
                lvgl::lv_label_long_mode_t_LV_LABEL_LONG_MODE_WRAP,
            );
            lvgl::lv_obj_set_width(info_label, lvgl::lv_pct(100));
        }

        Ok(tab_container)
    }

    pub async fn handle_event(&mut self, _event: &Event) -> bool {
        // For now, the general tab doesn't handle any specific events
        false
    }
}
