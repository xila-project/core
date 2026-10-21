use core::{ffi::c_void, mem::forget};

use crate::{
    error::{Error, Result},
    icon::create_icon,
    shortcut::{SHORTCUT_PATH, Shortcut},
};
use alloc::{
    collections::btree_map::BTreeMap,
    ffi::CString,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use xila::graphics::{self, Color, EventKind, Logo, OwnedWindow, Point, lvgl};
use xila::log::{self, error, warning};
use xila::virtual_file_system::Directory;
use xila::{executable, graphics::theme};
use xila::{executable::Standard, graphics::Event};
use xila::{file_system::Kind, graphics::symbol};

pub const WINDOWS_PARENT_CHILD_CHANGED: graphics::EventKind = graphics::EventKind::Custom2;

pub struct Desk {
    context: &'static xila::executable::ExecutableContext,
    graphics_manager: &'static graphics::Manager,
    window: OwnedWindow,
    tile_view: *mut lvgl::lv_obj_t,
    drawer_tile: *mut lvgl::lv_obj_t,
    desk_tile: *mut lvgl::lv_obj_t,
    dock: *mut lvgl::lv_obj_t,
    main_button: *mut lvgl::lv_obj_t,
    dock_menu: Option<*mut lvgl::lv_obj_t>,
    dock_menu_target_window: Option<usize>,
    dock_menu_maximize_button: Option<*mut lvgl::lv_obj_t>,
    dock_menu_close_button: Option<*mut lvgl::lv_obj_t>,
    shortcuts: BTreeMap<*mut lvgl::lv_obj_t, String>,
}

unsafe extern "C" fn event_handler(event: *mut lvgl::lv_event_t) {
    unsafe {
        let raw_code = lvgl::lv_event_get_code(event);
        let code = EventKind::from_lvgl_code(raw_code);

        if code == EventKind::ChildCreated || code == EventKind::ChildDeleted {
            let target = lvgl::lv_event_get_target(event) as *mut lvgl::lv_obj_t;
            let target_parent = lvgl::lv_obj_get_parent(target);

            let current_target = lvgl::lv_event_get_current_target(event) as *mut lvgl::lv_obj_t;

            // If the event is not for the current target, ignore it (not the parent window)
            if target_parent != current_target {
                return;
            }

            let desk = lvgl::lv_event_get_user_data(event) as *mut lvgl::lv_obj_t;

            lvgl::lv_obj_send_event(
                desk,
                WINDOWS_PARENT_CHILD_CHANGED as u32,
                target as *mut c_void,
            );
        }
    }
}

impl Desk {
    const DOCK_ICON_SIZE: Point = Point::new(40, 40);
    const DRAWER_ICON_SIZE: Point = Point::new(48, 48);

    pub const HOME_EVENT: EventKind = EventKind::Custom1;

    pub fn get_window_object(&mut self) -> *mut lvgl::lv_obj_t {
        self.window.as_object_mutable()
    }

    pub fn is_hidden(&self) -> bool {
        unsafe { lvgl::lv_obj_has_flag(self.dock, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN) }
    }

    pub async fn new(
        context: &'static xila::executable::ExecutableContext,
        windows_parent: *mut lvgl::lv_obj_t,
    ) -> Result<Self> {
        let graphics = graphics::ffi_manager();
        let desk = graphics::lock!(graphics, {
            // - Create a window
            let mut window = graphics.create_window().await?;

            window.set_icon("De", Color::BLACK);

            unsafe {
                // Edge-to-edge window content: the shared window class already
                // flattens border and radius, only the container padding remains.
                lvgl::lv_obj_set_style_pad_all(window.as_mut(), 0, lvgl::LV_STATE_DEFAULT);

                lvgl::lv_obj_add_event_cb(
                    windows_parent,
                    Some(event_handler),
                    EventKind::All as u32,
                    window.as_object_mutable() as *mut _ as *mut _,
                );
            }

            // - Create the logo
            unsafe {
                // Create the logo in the background of the window
                let logo = Logo::new(window.as_mut(), 4, Color::BLACK)?;
                let logo_inner_object = logo.get_inner_object();
                forget(logo); // Prevent the logo from being dropped

                lvgl::lv_obj_set_align(logo_inner_object, lvgl::lv_align_t_LV_ALIGN_CENTER);
                lvgl::lv_obj_add_flag(
                    logo_inner_object,
                    lvgl::lv_obj_flag_t_LV_OBJ_FLAG_OVERFLOW_VISIBLE,
                );

                for i in 0..4 {
                    let part = lvgl::lv_obj_get_child(logo_inner_object, i);

                    lvgl::lv_obj_set_style_bg_opa(part, lvgl::LV_OPA_0, lvgl::LV_STATE_DEFAULT);

                    // The logo has borderless local styles; let the shared outline
                    // variant draw this desktop-specific outline instead.
                    lvgl::lv_obj_remove_local_style_prop(
                        part,
                        lvgl::_lv_style_id_t_LV_STYLE_BORDER_WIDTH as lvgl::lv_style_prop_t,
                        lvgl::LV_STATE_DEFAULT,
                    );
                    lvgl::lv_theme_shadcn_set_variant(
                        part,
                        lvgl::lv_theme_shadcn_variant_t_LV_THEME_SHADCN_VARIANT_OUTLINE,
                    );
                }
            }

            // - Create a tile view
            let tile_view = unsafe {
                let tile_view = lvgl::lv_tileview_create(window.as_mut());

                if tile_view.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_obj_set_style_bg_opa(tile_view, lvgl::LV_OPA_0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_scrollbar_mode(
                    tile_view,
                    lvgl::lv_scrollbar_mode_t_LV_SCROLLBAR_MODE_OFF,
                );

                tile_view
            };

            // - Create the desk tile
            let desk_tile = unsafe {
                let desk =
                    lvgl::lv_tileview_add_tile(tile_view, 0, 0, lvgl::lv_dir_t_LV_DIR_BOTTOM);

                if desk.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                desk
            };

            // - Create the drawer tile
            let drawer_tile = unsafe {
                let drawer = lvgl::lv_tileview_add_tile(tile_view, 0, 1, lvgl::lv_dir_t_LV_DIR_TOP);

                if drawer.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_obj_set_flex_flow(drawer, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW_WRAP);
                lvgl::lv_obj_set_flex_align(
                    drawer,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                );
                lvgl::lv_obj_set_style_pad_all(drawer, 24, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_pad_row(drawer, 20, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_pad_column(drawer, 20, lvgl::LV_STATE_DEFAULT);
                theme::set_surface(drawer, theme::Surface::Muted);

                drawer
            };

            // - Create a dock
            let dock = unsafe {
                let dock = lvgl::lv_obj_create(desk_tile);

                if dock.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                theme::set_surface(dock, theme::Surface::Muted);

                lvgl::lv_obj_set_align(dock, lvgl::lv_align_t_LV_ALIGN_BOTTOM_MID);
                lvgl::lv_obj_set_size(dock, lvgl::LV_SIZE_CONTENT, lvgl::LV_SIZE_CONTENT);
                lvgl::lv_obj_set_flex_flow(dock, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
                lvgl::lv_obj_set_style_pad_all(dock, 10, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_pad_column(dock, 8, lvgl::LV_STATE_DEFAULT);
                // Floating dock bar: the theme container style supplies the card
                // padding, radius, border and icon gap; only the surface is set.
                // Center items so the smaller main button aligns with the icons.
                lvgl::lv_obj_set_flex_align(
                    dock,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                );

                dock
            };

            // - Create the main button
            let main_button = unsafe {
                let logo = Logo::new(dock, 1, Color::WHITE)?;
                let inner_object = logo.get_inner_object();
                forget(logo); // Prevent the logo from being dropped
                inner_object
            };

            let shortcuts = BTreeMap::new();

            let desk: Desk = Self {
                context,
                graphics_manager: graphics::ffi_manager(),
                window,
                tile_view,
                desk_tile,
                drawer_tile,
                dock,
                main_button,
                dock_menu: None,
                dock_menu_target_window: None,
                dock_menu_maximize_button: None,
                dock_menu_close_button: None,
                shortcuts,
            };

            desk
        });

        Ok(desk)
    }

    unsafe fn create_drawer_shortcut(
        &mut self,
        entry_name: &str,
        name: &str,
        icon_color: Color,
        icon_string: &str,
        drawer: *mut lvgl::lv_obj_t,
    ) -> Result<()> {
        let icon = unsafe {
            let container = lvgl::lv_obj_create(drawer);

            lvgl::lv_obj_set_size(container, 112, 108);
            lvgl::lv_obj_set_flex_flow(container, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_style_pad_all(container, 10, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_pad_row(container, 8, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_flex_align(
                container,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_EVENLY,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
            );
            theme::set_surface(container, theme::Surface::Card);

            let icon = create_icon(container, icon_color, icon_string, Self::DRAWER_ICON_SIZE)?;

            let label = lvgl::lv_label_create(container);

            if label.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            let name = CString::new(name).map_err(Error::NullCharacterInString)?;

            lvgl::lv_label_set_text(label, name.as_ptr());
            lvgl::lv_label_set_long_mode(label, lvgl::lv_label_long_mode_t_LV_LABEL_LONG_MODE_WRAP);
            lvgl::lv_obj_set_width(label, lvgl::lv_pct(100));
            lvgl::lv_obj_set_style_text_align(
                label,
                lvgl::lv_text_align_t_LV_TEXT_ALIGN_CENTER,
                lvgl::LV_STATE_DEFAULT,
            );

            icon
        };

        self.shortcuts.insert(icon, entry_name.to_string());

        Ok(())
    }

    async unsafe fn create_drawer_interface(&mut self, drawer: *mut lvgl::lv_obj_t) -> Result<()> {
        unsafe {
            let task = self
                .context
                .task_manager
                .get_current_task_identifier()
                .await;

            let virtual_file_system = &self.context.virtual_file_system;

            let _ = virtual_file_system
                .create_directory(task, &SHORTCUT_PATH)
                .await;

            let mut buffer: Vec<u8> = vec![];

            let shortcuts_directory = Directory::open(virtual_file_system, task, SHORTCUT_PATH)
                .await
                .map_err(Error::FailedToReadShortcutDirectory)?;

            for shortcut_entry in shortcuts_directory {
                if shortcut_entry.kind != Kind::File {
                    continue;
                }

                if !shortcut_entry.name.ends_with(".json") {
                    continue;
                }

                match Shortcut::read(self.context, &shortcut_entry.name, &mut buffer).await {
                    Ok(shortcut) => {
                        self.create_drawer_shortcut(
                            &shortcut_entry.name,
                            &shortcut.name,
                            shortcut.get_icon_color(),
                            &shortcut.icon_string,
                            drawer,
                        )?;
                    }
                    Err(e) => {
                        error!("Failed to read shortcut {}: {e:?}", shortcut_entry.name);
                        continue;
                    }
                }
            }

            Ok(())
        }
    }

    async fn execute_shortcut(&mut self, shortcut_name: &str) -> Result<()> {
        let task = self
            .context
            .task_manager
            .get_current_task_identifier()
            .await;

        let mut buffer = vec![];

        let shortcut = Shortcut::read(self.context, shortcut_name, &mut buffer).await?;

        let standard = Standard::open(
            &"/devices/standard_io/null",
            &"/devices/standard_io/null",
            &"/devices/standard_io/null",
            task,
            self.context.virtual_file_system,
            self.context,
        )
        .await
        .map_err(Error::FailedToOpenStandardFile)?;

        executable::execute(
            self.context,
            &*shortcut.command,
            shortcut.arguments,
            standard,
            None,
        )
        .await
        .map_err(Error::FailedToExecuteShortcut)?;

        Ok(())
    }

    unsafe fn open_dock_menu(
        &mut self,
        _dock_icon: *mut lvgl::lv_obj_t,
        window_identifier: usize,
    ) -> Result<()> {
        unsafe {
            self.close_dock_menu();

            let menu = lvgl::lv_list_create(self.window.as_object_mutable());

            if menu.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_set_size(menu, 160, lvgl::LV_SIZE_CONTENT);

            lvgl::lv_obj_align_to(
                menu,
                _dock_icon,
                lvgl::lv_align_t_LV_ALIGN_OUT_TOP_MID,
                0,
                0,
            );
            lvgl::lv_obj_set_scrollbar_mode(menu, lvgl::lv_scrollbar_mode_t_LV_SCROLLBAR_MODE_OFF);

            let maximize_button =
                lvgl::lv_list_add_button(menu, symbol::UP.as_ptr() as _, c"Maximize".as_ptr());

            if maximize_button.is_null() {
                lvgl::lv_obj_delete_async(menu);
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_set_width(maximize_button, lvgl::lv_pct(100));

            let close_button =
                lvgl::lv_list_add_button(menu, symbol::CLOSE.as_ptr() as _, c"Close".as_ptr());

            if close_button.is_null() {
                lvgl::lv_obj_delete_async(menu);
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_set_width(close_button, lvgl::lv_pct(100));

            self.dock_menu_maximize_button = Some(maximize_button);
            self.dock_menu = Some(menu);
            self.dock_menu_close_button = Some(close_button);
            self.dock_menu_target_window = Some(window_identifier);

            Ok(())
        }
    }

    unsafe fn close_dock_menu(&mut self) {
        unsafe {
            if let Some(menu) = self.dock_menu.take() {
                lvgl::lv_obj_delete_async(menu);
            }

            self.dock_menu_target_window = None;
            self.dock_menu_maximize_button = None;
            self.dock_menu_close_button = None;
        }
    }

    async fn close_app_for_window(&mut self, window_identifier: usize) {
        if let Err(error) = self
            .graphics_manager
            .send_window_close_request(window_identifier)
            .await
        {
            warning!(
                "Failed to send close request to window {}: {:?}",
                window_identifier,
                error
            );
        }
    }

    unsafe fn clear_main_button_pressed_state(&self) {
        unsafe {
            const STATE: lvgl::lv_state_t = lvgl::LV_STATE_PRESSED;

            lvgl::lv_obj_remove_state(self.main_button, STATE);

            for i in 0..4 {
                let part = lvgl::lv_obj_get_child(self.main_button, i);

                lvgl::lv_obj_remove_state(part, STATE);
            }
        }
    }

    unsafe fn is_object_inside(parent: *mut lvgl::lv_obj_t, object: *mut lvgl::lv_obj_t) -> bool {
        unsafe {
            let mut current = object;

            while !current.is_null() {
                if current == parent {
                    return true;
                }

                current = lvgl::lv_obj_get_parent(current);
            }

            false
        }
    }

    async fn get_visible_windows(&self) -> Result<Vec<(usize, String, Color)>> {
        let graphics_manager = self.graphics_manager;
        let window_count = graphics_manager.get_window_count().await?;

        let mut windows = Vec::new();

        for index in 0..window_count {
            let Ok(window_identifier) = graphics_manager.get_window_identifier(index).await else {
                continue;
            };

            if window_identifier == self.window.get_identifier() {
                continue;
            }

            match graphics_manager.get_window_icon(index).await {
                Ok((icon_string, icon_color)) => {
                    windows.push((window_identifier, icon_string, icon_color));
                }
                Err(error) => {
                    warning!(
                        "Failed to fetch icon for window {} during dock rebuild: {:?}",
                        window_identifier,
                        error
                    );
                }
            }
        }

        Ok(windows)
    }

    unsafe fn clear_dock_icons(&self) {
        unsafe {
            let mut child_index = lvgl::lv_obj_get_child_count(self.dock) as i32;

            while child_index > 0 {
                child_index -= 1;

                let child = lvgl::lv_obj_get_child(self.dock, child_index);

                if child.is_null() || child == self.main_button {
                    continue;
                }

                lvgl::lv_obj_delete(child);
            }
        }
    }

    unsafe fn rebuild_dock_icons(&self, windows: &[(usize, String, Color)]) -> Result<()> {
        unsafe {
            for (window_identifier, icon_string, icon_color) in windows {
                let icon = create_icon(self.dock, *icon_color, icon_string, Self::DOCK_ICON_SIZE)?;

                lvgl::lv_obj_set_user_data(icon, *window_identifier as *mut c_void);
            }
        }

        Ok(())
    }

    // This function is intentionally private and is only used within this module.
    async fn refresh_dock(&mut self) -> Result<()> {
        let visible_windows = self.get_visible_windows().await?;

        unsafe {
            self.clear_dock_icons();
            self.rebuild_dock_icons(&visible_windows)?;
        }

        if self
            .dock_menu_target_window
            .is_some_and(|window_identifier| {
                !visible_windows
                    .iter()
                    .any(|(visible_identifier, _, _)| *visible_identifier == window_identifier)
            })
        {
            unsafe {
                self.close_dock_menu();
            }
        }

        Ok(())
    }

    pub async fn handle_events(&mut self) -> bool {
        graphics::lock!(self.graphics_manager, {
            while let Some(event) = self.window.pop_event() {
                if let Err(error) = self.handle_event(event).await {
                    log::error!("Failed to handle desk event: {error:?}");
                }
            }
        });

        true
    }

    pub async fn handle_event(&mut self, event: Event) -> Result<()> {
        match event.code {
            Self::HOME_EVENT => unsafe {
                lvgl::lv_tileview_set_tile_by_index(self.tile_view, 0, 0, true);
            },
            EventKind::ValueChanged if event.target == self.tile_view => unsafe {
                if lvgl::lv_tileview_get_tile_active(self.tile_view) == self.desk_tile {
                    lvgl::lv_obj_clean(self.drawer_tile);
                } else if lvgl::lv_obj_get_child_count(self.drawer_tile) == 0 {
                    let _ = self.create_drawer_interface(self.drawer_tile).await;
                }
            },
            EventKind::Clicked => {
                if self
                    .dock_menu_maximize_button
                    .is_some_and(|maximize_button| {
                        event.target == maximize_button
                            || unsafe { lvgl::lv_obj_get_parent(event.target) == maximize_button }
                    })
                {
                    if let Some(window_identifier) = self.dock_menu_target_window
                        && let Err(error) = self
                            .graphics_manager
                            .maximize_window(window_identifier)
                            .await
                    {
                        warning!(
                            "Failed to maximize window {} from dock menu: {:?}",
                            window_identifier,
                            error
                        );
                    }

                    unsafe {
                        self.close_dock_menu();
                    }

                    return Ok(());
                }

                if self.dock_menu_close_button.is_some_and(|close_button| {
                    event.target == close_button
                        || unsafe { lvgl::lv_obj_get_parent(event.target) == close_button }
                }) {
                    if let Some(window_identifier) = self.dock_menu_target_window {
                        self.close_app_for_window(window_identifier).await;
                    }

                    unsafe {
                        self.close_dock_menu();
                    }

                    return Ok(());
                }

                if self.dock_menu.is_some()
                    && unsafe { lvgl::lv_obj_get_parent(event.target) == self.dock }
                    && event.target != self.main_button
                {
                    let clicked_window_identifier =
                        unsafe { lvgl::lv_obj_get_user_data(event.target) as usize };

                    if self
                        .dock_menu_target_window
                        .is_some_and(|window_identifier| {
                            window_identifier == clicked_window_identifier
                        })
                    {
                        return Ok(());
                    }
                }

                if let Some(menu) = self.dock_menu
                    && !unsafe { Self::is_object_inside(menu, event.target) }
                {
                    unsafe {
                        self.close_dock_menu();
                    }
                }

                // If the target is a shortcut, execute the shortcut
                if let Some(shortcut_name) = self.shortcuts.get(&event.target).cloned() {
                    self.execute_shortcut(&shortcut_name).await?;
                }
                // If the target is a dock icon, move the window to the foreground
                else if unsafe { lvgl::lv_obj_get_parent(event.target) == self.dock } {
                    // Ignore the main button
                    if event.target == self.main_button {
                        return Ok(());
                    }

                    unsafe {
                        self.close_dock_menu();
                    }

                    let window_identifier =
                        unsafe { lvgl::lv_obj_get_user_data(event.target) as usize };

                    if let Err(error) = self
                        .graphics_manager
                        .maximize_window(window_identifier)
                        .await
                    {
                        warning!(
                            "Failed to maximize window {} from dock click: {:?}",
                            window_identifier,
                            error
                        );
                    }
                }
            }
            EventKind::LongPressed
                if unsafe { lvgl::lv_obj_get_parent(event.target) == self.dock }
                    && event.target != self.main_button =>
            {
                let window_identifier =
                    unsafe { lvgl::lv_obj_get_user_data(event.target) as usize };

                unsafe {
                    self.open_dock_menu(event.target, window_identifier)?;
                }
            }
            EventKind::Pressed
                if (event.target == self.main_button
                    || unsafe { lvgl::lv_obj_get_parent(event.target) == self.main_button }) =>
            unsafe {
                lvgl::lv_obj_add_state(self.main_button, lvgl::LV_STATE_PRESSED);
                for i in 0..4 {
                    let part = lvgl::lv_obj_get_child(self.main_button, i);

                    lvgl::lv_obj_add_state(part, lvgl::LV_STATE_PRESSED);
                }
            },
            EventKind::Released
                if (event.target == self.main_button
                    || unsafe { lvgl::lv_obj_get_parent(event.target) == self.main_button }) =>
            {
                unsafe {
                    self.clear_main_button_pressed_state();
                }

                unsafe {
                    lvgl::lv_tileview_set_tile_by_index(self.tile_view, 0, 1, true);
                }
            }
            EventKind::PressLost => unsafe {
                self.clear_main_button_pressed_state();
            },
            WINDOWS_PARENT_CHILD_CHANGED => {
                // Ignore consecutive windows parent child changed events
                if let Some(Event {
                    code: WINDOWS_PARENT_CHILD_CHANGED,
                    ..
                }) = self.window.peek_event()
                {
                    return Ok(());
                }

                if let Err(error) = self.refresh_dock().await {
                    warning!(
                        "Failed to refresh dock after window tree update: {:?}",
                        error
                    );
                }
            }
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, vec, vec::Vec};

    use xila::graphics::Color;

    fn visible_window_identifiers(windows: &[(usize, String, Color)]) -> Vec<usize> {
        windows
            .iter()
            .map(|(window_identifier, _, _)| *window_identifier)
            .collect()
    }

    #[test]
    fn preserves_original_order_of_non_desk_windows() {
        let windows = vec![
            (42, String::from("A"), Color::BLACK),
            (77, String::from("B"), Color::WHITE),
            (3, String::from("C"), Color::new(12, 34, 56)),
        ];

        assert_eq!(visible_window_identifiers(&windows), vec![42, 77, 3]);
    }

    #[test]
    fn allows_detecting_disappeared_menu_target() {
        let windows = vec![
            (2, String::from("A"), Color::BLACK),
            (3, String::from("B"), Color::WHITE),
        ];

        let visible = visible_window_identifiers(&windows);

        assert!(!visible.contains(&99));
    }
}
