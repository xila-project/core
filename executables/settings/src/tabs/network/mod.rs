mod interface;

use crate::{Error, Result};
use alloc::{ffi::CString, string::ToString};
use core::{ffi::CStr, ptr::null_mut, time::Duration};
use interface::*;
use xila::{
    file_system::{AccessFlags, Path},
    graphics::{Event, EventKind, lvgl, symbol},
    log,
    network::{self, InterfaceKind},
    virtual_file_system::{Directory, File, VirtualFileSystem},
};

pub struct NetworkTab {
    context: &'static xila::executable::ExecutableContext,
    tab_container: *mut lvgl::lv_obj_t,
    interfaces_list: *mut lvgl::lv_obj_t,
    last_update: Duration,
    configuration_panel: Option<InterfacePanel>,
    parent_tabview: *mut lvgl::lv_obj_t,
}

impl NetworkTab {
    pub const UPDATE_INTERVAL: Duration = Duration::from_secs(30);

    pub fn new(context: &'static xila::executable::ExecutableContext) -> Self {
        Self {
            context,
            tab_container: null_mut(),
            interfaces_list: null_mut(),
            last_update: Duration::from_secs(0),
            configuration_panel: None,
            parent_tabview: null_mut(),
        }
    }

    pub async fn get_interface_kind_status(
        &self,
        interface_name: &str,
    ) -> Result<(InterfaceKind, bool)> {
        let virtual_file_system = &self.context.virtual_file_system;

        let mut file = open_interface(virtual_file_system, interface_name).await?;

        let kind = file.control(network::GET_KIND, &()).await?;
        let is_up = file.control(network::IS_LINK_UP, &()).await?;

        file.close(virtual_file_system).await?;

        Ok((kind, is_up))
    }
    pub async fn update_interfaces(&mut self) -> Result<()> {
        // Clear existing list items
        unsafe {
            lvgl::lv_obj_clean(self.interfaces_list);
        }

        let virtual_file_system = &self.context.virtual_file_system;

        let task_manager = &self.context.task_manager;

        let task = task_manager.get_current_task_identifier().await;

        let mut directory =
            Directory::open(virtual_file_system, task, Path::NETWORK_DEVICES).await?;

        while let Some(entry) = directory.read().await? {
            if entry.name == "." || entry.name == ".." {
                continue;
            }

            let (kind, is_up) = self.get_interface_kind_status(&entry.name).await?;

            let label_text =
                CString::new(entry.name.as_str()).map_err(|_| Error::FailedToCreateUiElement)?;

            let symbol = match kind {
                InterfaceKind::Ethernet => symbol::NETWORK_WIRED,
                InterfaceKind::WiFi => symbol::WIFI,
                InterfaceKind::Unknown => c"?",
            };

            unsafe {
                let button = lvgl::lv_list_add_button(
                    self.interfaces_list,
                    symbol.as_ptr() as _,
                    label_text.as_ptr() as *const _,
                );

                // Theme list-button style owns padding and gap; center content
                lvgl::lv_obj_set_flex_align(
                    button,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                );

                let switch = lvgl::lv_switch_create(button);

                log::information!(
                    "Interface {} is {:?}, link is {}",
                    entry.name,
                    kind,
                    if is_up { "up" } else { "down" }
                );

                lvgl::lv_obj_set_state(switch, lvgl::LV_STATE_CHECKED as _, is_up);
            }
        }

        Ok(())
    }

    pub async fn create_ui(
        &mut self,
        parent_tabview: *mut lvgl::lv_obj_t,
    ) -> crate::error::Result<*mut lvgl::lv_obj_t> {
        self.parent_tabview = parent_tabview;

        self.tab_container =
            unsafe { lvgl::lv_tabview_add_tab(parent_tabview, c"Network".as_ptr() as *const _) };

        if self.tab_container.is_null() {
            return Err(crate::error::Error::FailedToCreateUiElement);
        }

        // Flex column layout so children can use flex_grow to fill the tile
        unsafe {
            lvgl::lv_obj_set_flex_flow(
                self.tab_container,
                lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN,
            );
            lvgl::lv_obj_set_style_pad_all(self.tab_container, 16, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_pad_row(self.tab_container, 10, lvgl::LV_STATE_DEFAULT);
        }

        // Create interface list

        unsafe {
            self.interfaces_list = lvgl::lv_list_create(self.tab_container);
            if self.interfaces_list.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            // File list properties - use flex grow to fill remaining space
            lvgl::lv_obj_set_width(self.interfaces_list, lvgl::lv_pct(100));
            lvgl::lv_obj_set_flex_grow(self.interfaces_list, 1); // Take remaining vertical space
            lvgl::lv_obj_set_style_pad_all(self.interfaces_list, 8, lvgl::LV_STATE_DEFAULT);
            xila::graphics::theme::set_surface(
                self.interfaces_list,
                xila::graphics::theme::Surface::Card,
            );
        }

        self.update_interfaces().await?;

        Ok(self.tab_container)
    }

    pub async fn handle_event(&mut self, event: &Event) -> bool {
        let time_manager = &self.context.time_manager;

        let current_time = match time_manager.get_current_time() {
            Ok(time) => time,
            Err(_) => {
                log::error!("Failed to get current time for network tab update");
                return false;
            }
        };

        if current_time - self.last_update >= Self::UPDATE_INTERVAL {
            if let Err(e) = self.update_interfaces().await {
                log::error!("Failed to update network interfaces: {:?}", e);
            }

            self.last_update = current_time;
        }

        if let Some(panel) = &mut self.configuration_panel {
            if !panel.handle_event(event).await {
                self.configuration_panel.take();
            }

            return true;
        }

        // check if any specific events need to be handled here
        if event.code == EventKind::Clicked {
            // Find which interface button was clicked
            let parent = unsafe { lvgl::lv_obj_get_parent(event.target) };
            if parent == self.interfaces_list
                && unsafe { lvgl::lv_obj_check_type(event.target, &lvgl::lv_list_button_class) }
            {
                let interface = unsafe {
                    let label = lvgl::lv_obj_get_child(event.target, 1);

                    if label.is_null() {
                        log::error!("Failed to get label child from interface button");
                        return false;
                    }

                    let text = lvgl::lv_label_get_text(label);

                    if text.is_null() {
                        log::error!("Failed to get text from label");
                        return false;
                    }

                    CStr::from_ptr(text as *const _)
                        .to_str()
                        .unwrap_or_default()
                };

                log::information!("Opening configuration panel for interface: {}", interface);

                match InterfacePanel::new(self.context, interface.to_string(), self.tab_container)
                    .await
                {
                    Ok(panel) => {
                        self.configuration_panel.replace(panel);
                    }
                    Err(e) => {
                        log::error!("Failed to create configuration panel: {:?}", e);
                    }
                }

                return true;
            }
        }

        false
    }
}

pub async fn open_interface<'a>(
    virtual_file_system: &'a VirtualFileSystem,
    interface: &str,
) -> Result<File<'a>> {
    let task_manager = virtual_file_system.task_manager();

    let task = task_manager.get_current_task_identifier().await;

    let path = Path::NETWORK_DEVICES
        .join(Path::from_str(interface))
        .ok_or(Error::FailedToCreateUiElement)?;

    let file = File::open(
        virtual_file_system,
        task,
        &path,
        AccessFlags::READ_WRITE.into(),
    )
    .await?;

    Ok(file)
}
