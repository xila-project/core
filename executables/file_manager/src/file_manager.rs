use crate::error::{Error, Result};
pub(crate) use alloc::{
    borrow::ToOwned,
    ffi::CString,
    string::{String, ToString},
    vec::Vec,
};
use core::ptr::null_mut;
use xila::log;
use xila::task;
use xila::virtual_file_system::Directory;
use xila::{
    file_system::{Kind, Path, PathOwned},
    graphics::{
        self, EventKind, lvgl,
        palette::{self, Hue},
    },
};
use xila::{graphics::OwnedWindow, internationalization::translate};

pub struct FileManager {
    context: &'static xila::executable::ExecutableContext,
    graphics_manager: &'static graphics::Manager,
    window: OwnedWindow,
    toolbar: *mut lvgl::lv_obj_t,
    up_button: *mut lvgl::lv_obj_t,
    home_button: *mut lvgl::lv_obj_t,
    refresh_button: *mut lvgl::lv_obj_t,
    path_text_area: *mut lvgl::lv_obj_t,
    go_button: *mut lvgl::lv_obj_t,
    file_list: *mut lvgl::lv_obj_t,
    current_path: PathOwned,
    files: Vec<FileItem>,
    running: bool,
}

#[derive(Clone)]
pub struct FileItem {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

impl FileManager {
    pub async fn new(context: &'static xila::executable::ExecutableContext) -> Result<Self> {
        let graphics_manager = graphics::ffi_manager();
        let manager = graphics::lock!(graphics_manager, {
            let mut window = graphics_manager.create_window().await?;

            window.set_icon("Fm", palette::get(Hue::Cyan, palette::Tone::MAIN));

            let mut manager = Self {
                context,
                graphics_manager,
                window,
                toolbar: null_mut(),
                up_button: null_mut(),
                home_button: null_mut(),
                refresh_button: null_mut(),
                path_text_area: null_mut(),
                go_button: null_mut(),
                file_list: null_mut(),
                current_path: PathOwned::root(),
                files: Vec::new(),
                running: true,
            };

            // Set up window layout for flex
            unsafe {
                lvgl::lv_obj_set_layout(
                    manager.window.as_object_mutable(),
                    lvgl::lv_layout_t_LV_LAYOUT_FLEX,
                );
                lvgl::lv_obj_set_flex_flow(
                    manager.window.as_object_mutable(),
                    lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN,
                );
                lvgl::lv_obj_set_style_pad_all(
                    manager.window.as_object_mutable(),
                    0,
                    lvgl::LV_STATE_DEFAULT,
                );
            }

            manager.create_toolbar().await?;
            manager.create_file_list().await?;
            manager.load_directory().await?;

            manager
        });

        Ok(manager)
    }

    pub async fn handle_events(&mut self) -> bool {
        graphics::lock!(self.graphics_manager, {
            while let Some(event) = self.window.pop_event() {
                if let Err(e) = self.handle_event(event).await {
                    log::error!(translate!("errors.event_handling_failed"), e);
                }
            }
        });

        task::suspend(|ctx| {
            self.window.register_waker(ctx.waker());
        })
        .await;

        self.running
    }

    pub async fn handle_event(&mut self, event: graphics::Event) -> Result<()> {
        match event.code {
            EventKind::Delete | EventKind::CloseRequested
                if event.target == self.window.as_object_mutable() =>
            {
                self.running = false;
            }
            EventKind::Clicked => {
                let target = event.target;

                // Handle different button clicks
                if target == self.up_button {
                    self.handle_up_click().await?;
                } else if target == self.home_button {
                    self.handle_home_click().await?;
                } else if target == self.refresh_button {
                    self.handle_refresh_click().await?;
                } else if target == self.go_button {
                    self.handle_go_click().await?;
                } else {
                    // Handle file item clicks
                    self.handle_file_click(target).await?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn create_toolbar(&mut self) -> Result<()> {
        unsafe {
            self.toolbar = lvgl::lv_obj_create(self.window.as_object_mutable());
            if self.toolbar.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            // Toolbar - content-height flex child of the window's column layout,
            // flattened to match the window's edge-to-edge chrome
            lvgl::lv_obj_set_size(self.toolbar, lvgl::lv_pct(100), lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_style_border_width(self.toolbar, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_radius(self.toolbar, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_layout(self.toolbar, lvgl::lv_layout_t_LV_LAYOUT_FLEX);
            lvgl::lv_obj_set_flex_flow(self.toolbar, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
            lvgl::lv_obj_set_flex_align(
                self.toolbar,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_START,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
            );

            // Up button
            self.up_button = lvgl::lv_button_create(self.toolbar);
            if self.up_button.is_null() {
                return Err(Error::FailedToCreateObject);
            }
            let up_label = lvgl::lv_label_create(self.up_button);
            lvgl::lv_label_set_text(
                up_label,
                lvgl::LV_SYMBOL_UP as *const _ as *const core::ffi::c_char,
            );
            lvgl::lv_obj_center(up_label);

            // Remove event handler - events bubble up to window

            // Home button
            self.home_button = lvgl::lv_button_create(self.toolbar);
            if self.home_button.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            let home_label = lvgl::lv_label_create(self.home_button);
            lvgl::lv_label_set_text(
                home_label,
                lvgl::LV_SYMBOL_HOME as *const _ as *const core::ffi::c_char,
            );
            lvgl::lv_obj_center(home_label);

            // Remove event handler - events bubble up to window

            // Refresh button
            self.refresh_button = lvgl::lv_button_create(self.toolbar);
            if self.refresh_button.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            let refresh_label = lvgl::lv_label_create(self.refresh_button);

            lvgl::lv_label_set_text(
                refresh_label,
                lvgl::LV_SYMBOL_REFRESH as *const _ as *const core::ffi::c_char,
            );
            lvgl::lv_obj_center(refresh_label);

            // Remove event handler - events bubble up to window

            // Path text area - use flex grow to take remaining space
            self.path_text_area = lvgl::lv_textarea_create(self.toolbar);
            if self.path_text_area.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_set_flex_grow(self.path_text_area, 1); // Take remaining space

            // Configure text area properties
            lvgl::lv_textarea_set_one_line(self.path_text_area, true);
            lvgl::lv_textarea_set_cursor_click_pos(self.path_text_area, true);

            // Go button
            self.go_button = lvgl::lv_button_create(self.toolbar);
            if self.go_button.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            let go_label = lvgl::lv_label_create(self.go_button);
            lvgl::lv_label_set_text(
                go_label,
                lvgl::LV_SYMBOL_RIGHT as *const _ as *const core::ffi::c_char,
            );
            lvgl::lv_obj_center(go_label);

            self.update_path_label();
        }

        Ok(())
    }

    async fn create_file_list(&mut self) -> Result<()> {
        unsafe {
            self.file_list = lvgl::lv_list_create(self.window.as_object_mutable());
            if self.file_list.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            // File list properties - use flex grow to fill remaining space
            lvgl::lv_obj_set_width(self.file_list, lvgl::lv_pct(100));
            lvgl::lv_obj_set_flex_grow(self.file_list, 1); // Take remaining vertical space

            // Ensure proper scrolling behavior
            lvgl::lv_obj_set_style_pad_all(self.file_list, 0, lvgl::LV_STATE_DEFAULT);
        }

        Ok(())
    }

    async fn load_directory(&mut self) -> Result<()> {
        // Clear existing files
        self.clear_file_list();

        let task = self
            .context
            .task_manager
            .get_current_task_identifier()
            .await;

        // Open directory
        let virtual_file_system = &self.context.virtual_file_system;

        let mut directory = Directory::open(virtual_file_system, task, &self.current_path).await?;

        // Read directory entries
        while let Some(entry) = directory.read().await? {
            let name = entry.name.clone();

            // Skip "." and ".." entries
            if name == "." || name == ".." {
                continue;
            }

            let file_item = FileItem {
                name: name.clone(),
                kind: entry.kind,
                size: entry.size,
            };

            self.files.push(file_item);
        }

        // Sort files: directories first, then files
        self.files.sort_by(|a, b| match (a.kind, b.kind) {
            (Kind::Directory, Kind::Directory) => a.name.cmp(&b.name),
            (Kind::Directory, _) => core::cmp::Ordering::Less,
            (_, Kind::Directory) => core::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        // Create UI for each file
        for i in 0..self.files.len() {
            self.create_file_item(i).await?;
        }

        Ok(())
    }

    async fn create_file_item(&mut self, index: usize) -> Result<()> {
        unsafe {
            let file = &self.files[index];

            let icon_symbol = match file.kind {
                Kind::Directory => lvgl::LV_SYMBOL_DIRECTORY,
                _ => lvgl::LV_SYMBOL_FILE,
            };

            let name_cstring = CString::new(file.name.clone()).unwrap();
            let button = lvgl::lv_list_add_button(
                self.file_list,
                icon_symbol.as_ptr() as *const core::ffi::c_void,
                name_cstring.as_ptr(),
            );

            if button.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            // Store the file index in the button's user data for identification
            lvgl::lv_obj_set_user_data(button, index as *mut core::ffi::c_void);
        }

        Ok(())
    }

    fn clear_file_list(&mut self) {
        unsafe {
            // Clear the list content - lvgl:: list automatically manages its children
            lvgl::lv_obj_clean(self.file_list);
        }
        self.files.clear();
    }

    fn update_path_label(&self) {
        unsafe {
            if !self.path_text_area.is_null() {
                let path_text = CString::new(self.current_path.to_string()).unwrap();
                lvgl::lv_textarea_set_text(self.path_text_area, path_text.as_ptr());
            } else {
                log::error!("Path text area is null!");
            }
        }
    }

    async fn handle_file_click(&mut self, target: *mut lvgl::lv_obj_t) -> Result<()> {
        unsafe {
            // Check if the clicked object is a file list button
            let parent = lvgl::lv_obj_get_parent(target as *const lvgl::lv_obj_t);
            if core::ptr::eq(parent as *mut lvgl::lv_obj_t, self.file_list) {
                // Get the file index from user data
                let index = lvgl::lv_obj_get_user_data(target) as usize;

                if index < self.files.len() {
                    let file = &self.files[index];

                    if file.kind == Kind::Directory {
                        // Navigate to directory
                        if let Some(new_path) =
                            self.current_path.clone().join(Path::from_str(&file.name))
                        {
                            self.current_path = new_path;
                            self.update_path_label();
                            // Reload directory contents
                            self.load_directory().await?;
                        }
                    } else {
                        // Handle file selection/opening
                        // Could open files with appropriate applications
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_up_click(&mut self) -> Result<()> {
        if let Some(parent_path) = self.current_path.go_parent() {
            self.current_path = parent_path.to_owned();
            self.update_path_label();
            self.load_directory().await?;
        }
        Ok(())
    }

    async fn handle_home_click(&mut self) -> Result<()> {
        self.current_path = PathOwned::root();
        self.update_path_label();
        self.load_directory().await?;
        Ok(())
    }

    async fn handle_refresh_click(&mut self) -> Result<()> {
        self.load_directory().await?;
        Ok(())
    }

    async fn handle_go_click(&mut self) -> Result<()> {
        unsafe {
            if !self.path_text_area.is_null() {
                // Get the text from the text area
                let text_ptr: *const core::ffi::c_char =
                    lvgl::lv_textarea_get_text(self.path_text_area);
                if !text_ptr.is_null() {
                    // Convert C string to Rust string
                    let text_cstr = core::ffi::CStr::from_ptr(text_ptr);
                    if let Ok(path_str) = text_cstr.to_str() {
                        // Try to create a path from the entered string
                        let new_path = PathOwned::new(path_str.to_string()).unwrap_or_else(|| {
                            log::error!(translate!("errors.invalid_entered_path"), path_str);
                            self.current_path.clone()
                        });

                        // Navigate to the new path
                        self.current_path = new_path;
                        self.update_path_label();

                        // Try to load the directory
                        self.load_directory().await?;
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn refresh(&mut self) -> Result<()> {
        self.load_directory().await
    }

    pub async fn navigate_to(&mut self, path: &Path) -> Result<()> {
        self.current_path = path.to_owned();
        self.update_path_label();
        self.load_directory().await
    }

    pub fn get_current_path(&self) -> &Path {
        &self.current_path
    }

    pub fn is_running(&self) -> bool {
        self.running
    }
}
