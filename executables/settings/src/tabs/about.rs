use crate::error::Result;
use alloc::{ffi::CString, format, string::ToString, vec::Vec};
use core::{ffi::CStr, ptr::null_mut, str};
use xila::{
    about,
    graphics::{
        Event,
        lvgl::{self},
    },
    internationalization::{self, translate},
    memory,
    shared::{BYTES_SUFFIX, Unit},
    virtual_file_system::File,
};

pub struct AboutTab {
    context: &'static xila::executable::ExecutableContext,
    container: *mut lvgl::lv_obj_t,
    list: *mut lvgl::lv_obj_t,
}

impl AboutTab {
    pub fn new(context: &'static xila::executable::ExecutableContext) -> Self {
        Self {
            context,
            container: null_mut() as *mut _,
            list: null_mut() as *mut _,
        }
    }

    pub async fn create_ui(
        &mut self,
        parent_tabview: *mut lvgl::lv_obj_t,
    ) -> Result<*mut lvgl::lv_obj_t> {
        self.container =
            unsafe { lvgl::lv_tabview_add_tab(parent_tabview, translate!(c"About").as_ptr()) };

        if self.container.is_null() {
            return Err(crate::error::Error::FailedToCreateUiElement);
        }

        // Create list
        unsafe {
            self.list = lvgl::lv_list_create(self.container);

            if self.list.is_null() {
                return Err(crate::error::Error::FailedToCreateUiElement);
            }

            // List properties - fill container
            lvgl::lv_obj_set_size(self.list, lvgl::lv_pct(100), lvgl::lv_pct(100));
            lvgl::lv_obj_set_style_pad_all(self.list, 0, lvgl::LV_STATE_DEFAULT);
        }

        // Populate items - convert CStr to String
        self.create_list_item(translate!(c"Operating System:"), c"Xila")?;

        let description = CString::new(about::get_description())
            .map_err(|_| crate::error::Error::FailedToCreateUiElement)?;
        self.create_list_item(translate!(c"Description:"), &description)?;

        let authors = CString::new(about::get_authors())
            .map_err(|_| crate::error::Error::FailedToCreateUiElement)?;
        self.create_list_item(translate!(c"Developed by:"), &authors)?;

        let license = CString::new(about::get_license())
            .map_err(|_| crate::error::Error::FailedToCreateUiElement)?;
        self.create_list_item(translate!(c"License:"), &license)?;

        let version = CString::new(about::get_version_string())
            .map_err(|_| crate::error::Error::FailedToCreateUiElement)?;
        self.create_list_item(translate!(c"Version:"), &version)?;

        let locale = CString::new(format!(
            "{} ({})",
            internationalization::get_locale(),
            internationalization::get_fallback_locale(),
        ))?;
        self.create_list_item(translate!(c"Locale:"), &locale)?;

        let memory = memory::global_manager().get_total_size();
        let memory = Unit::new(memory as f32, BYTES_SUFFIX.symbol);
        let memory = CString::new(format!("{}", memory))
            .map_err(|_| crate::error::Error::FailedToCreateUiElement)?;
        self.create_list_item(translate!(c"Memory:"), &memory)?;

        let cpu_summary = CString::new(self.get_cpu_summary().await)
            .map_err(|_| crate::error::Error::FailedToCreateUiElement)?;
        self.create_list_item(translate!(c"CPU:"), &cpu_summary)?;

        Ok(self.container)
    }

    async fn get_cpu_summary(&self) -> alloc::string::String {
        let virtual_file_system = &self.context.virtual_file_system;
        let task = self
            .context
            .task_manager
            .get_current_task_identifier()
            .await;

        let mut buffer = Vec::new();

        if File::read_from_path(
            virtual_file_system,
            task,
            "/devices/compute/informations",
            &mut buffer,
        )
        .await
        .is_err()
        {
            return "unknown (unknown cores, unknown)".to_string();
        }

        let content = str::from_utf8(&buffer).unwrap_or_default();

        let model_name = Self::get_cpu_info_value(content, "model name").unwrap_or("unknown");
        let cpu_cores = Self::get_cpu_info_value(content, "cpu cores").unwrap_or("unknown");
        let architecture = Self::get_cpu_info_value(content, "architecture").unwrap_or("unknown");

        format!("{} ({} cores, {})", model_name, cpu_cores, architecture)
    }

    fn get_cpu_info_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
        for line in content.lines() {
            let Some((line_key, line_value)) = line.split_once(':') else {
                continue;
            };

            if line_key.trim() == key {
                return Some(line_value.trim());
            }
        }

        None
    }

    fn create_list_item(&mut self, name: &CStr, value: &CStr) -> Result<()> {
        unsafe {
            lvgl::lv_list_add_text(self.list, name.as_ptr());

            let button = lvgl::lv_list_add_button(self.list, core::ptr::null(), value.as_ptr());

            if button.is_null() {
                return Err(crate::error::Error::FailedToCreateUiElement);
            }
        }

        Ok(())
    }

    pub async fn handle_event(&mut self, _event: &Event) -> bool {
        // For now, the about tab doesn't handle any specific events
        false
    }
}
