use crate::error::{Error, Result};
use alloc::string::String;
use core::ffi::CStr;
use core::ptr::null_mut;
use core::time::Duration;
use xila::file_system::{AccessFlags, Path};
use xila::graphics::{self, EventKind, lvgl, symbol, theme};
use xila::log;
use xila::network::InterfaceKind;
use xila::virtual_file_system::{Directory, File};
use xila::{internationalization, network};

const KEYBOARD_SIZE_RATIO: f64 = 3.0 / 1.0;

pub struct Layout {
    context: &'static xila::executable::ExecutableContext,
    window: *mut lvgl::lv_obj_t,
    _header: *mut lvgl::lv_obj_t,
    _body: *mut lvgl::lv_obj_t,
    clock: *mut lvgl::lv_obj_t,
    clock_string: String,
    _battery: *mut lvgl::lv_obj_t,
    network: *mut lvgl::lv_obj_t,
    last_update: Duration,
}

impl Drop for Layout {
    fn drop(&mut self) {
        unsafe { lvgl::lv_obj_delete(self.window) }
    }
}

/// Keyboard event handler.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
pub unsafe extern "C" fn keyboard_event_handler(event: *mut lvgl::lv_event_t) {
    unsafe {
        let code = lvgl::lv_event_get_code(event);
        let code = EventKind::from_lvgl_code(code);
        let keyboard = lvgl::lv_event_get_user_data(event) as *mut lvgl::lv_obj_t;

        if keyboard.is_null() {
            return;
        }

        match code {
            EventKind::Ready => {
                let focused_input = lvgl::lv_keyboard_get_textarea(keyboard);

                if focused_input.is_null() {
                    return;
                }

                lvgl::lv_obj_send_event(
                    focused_input,
                    EventKind::Ready.into_lvgl_code(),
                    null_mut(),
                );

                lvgl::lv_keyboard_set_textarea(keyboard, null_mut());
                lvgl::lv_obj_add_flag(keyboard, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            }
            EventKind::Cancel => {
                lvgl::lv_keyboard_set_textarea(keyboard, null_mut());
                lvgl::lv_obj_add_flag(keyboard, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            }

            _ => {}
        }
    }
}

/// Screen event handler.
///
/// # Safety
///
/// This function is unsafe because it dereferences raw pointers.
pub unsafe extern "C" fn screen_event_handler(event: *mut lvgl::lv_event_t) {
    unsafe {
        let code = lvgl::lv_event_get_code(event);
        let code = EventKind::from_lvgl_code(code);
        let target = lvgl::lv_event_get_target_obj(event);
        let keyboard = lvgl::lv_event_get_user_data(event) as *mut lvgl::lv_obj_t;

        if target.is_null() || keyboard.is_null() {
            return;
        }

        match code {
            EventKind::Focused if lvgl::lv_obj_has_class(target, &lvgl::lv_textarea_class) => {
                lvgl::lv_keyboard_set_textarea(keyboard, target);
                lvgl::lv_obj_remove_flag(keyboard, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
                lvgl::lv_obj_move_foreground(keyboard);

                let width = lvgl::lv_obj_get_width(keyboard);
                lvgl::lv_obj_set_height(keyboard, (width as f64 / KEYBOARD_SIZE_RATIO) as i32);
            }
            EventKind::Defocused if lvgl::lv_obj_has_class(target, &lvgl::lv_textarea_class) => {
                lvgl::lv_keyboard_set_textarea(keyboard, null_mut());
                lvgl::lv_obj_add_flag(keyboard, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
            }
            _ => {}
        }
    }
}

impl Layout {
    pub const UPDATE_INTERVAL: Duration = Duration::from_secs(30);

    pub async fn run(&mut self) {
        let current_time = match self.context.time_manager.get_current_time() {
            Ok(time) => time,
            Err(e) => {
                log::error!("Failed to get current time: {}", e);
                return;
            }
        };

        if current_time - self.last_update < Self::UPDATE_INTERVAL {
            return;
        }

        self.update_clock(current_time).await;

        if let Err(e) = self.update_network_icon().await {
            log::error!("Failed to update network icon: {}", e);
        }

        self.last_update = current_time;
    }

    async fn get_interface_symbol(&self, file: &mut File<'_>) -> Result<Option<&CStr>> {
        let kind = match file
            .as_synchronous_file_mut()
            .control(network::GET_KIND, &())
        {
            Ok(kind) => kind,
            // HTTP(S) clients are protocol endpoints, not network interfaces.
            Err(
                xila::virtual_file_system::Error::FileSystem(
                    xila::file_system::Error::UnsupportedOperation,
                )
                | xila::virtual_file_system::Error::UnsupportedOperation,
            ) => return Ok(None),
            Err(error) => return Err(Error::FailedToOpenDirectory(error)),
        };
        let is_up = file
            .as_synchronous_file_mut()
            .control(network::IS_LINK_UP, &())
            .map_err(Error::FailedToOpenDirectory)?;
        if !is_up {
            return Ok(None);
        }

        let symbol = match kind {
            InterfaceKind::WiFi => symbol::WIFI,
            InterfaceKind::Ethernet => symbol::NETWORK_WIRED,
            InterfaceKind::Unknown => c"?",
        };

        Ok(Some(symbol))
    }

    async fn get_network_symbol(&self) -> Result<&CStr> {
        // Browse the network interfaces in the /devices/network directory

        let virtual_file_system = &self.context.virtual_file_system;

        let task_manager = &self.context.task_manager;

        let task = task_manager.get_current_task_identifier().await;

        let mut directory = Directory::open(virtual_file_system, task, Path::NETWORK_DEVICES)
            .await
            .map_err(Error::FailedToOpenDirectory)?;

        while let Some(entry) = directory
            .read()
            .await
            .map_err(Error::FailedToOpenDirectory)?
        {
            if entry.name == "." || entry.name == ".." {
                continue;
            }

            let entry_path = entry.join_path(Path::NETWORK_DEVICES);

            if let Some(entry_path) = entry_path {
                let mut file = File::open(
                    virtual_file_system,
                    task,
                    &entry_path,
                    AccessFlags::Read.into(),
                )
                .await
                .map_err(Error::FailedToOpenDirectory)?;

                let symbol = self.get_interface_symbol(&mut file).await?;

                if let Some(symbol) = symbol {
                    return Ok(symbol);
                }
            }
        }

        Ok(c"")
    }

    async fn update_network_icon(&mut self) -> Result<()> {
        let symbol = self.get_network_symbol().await?;

        let graphics_manager = graphics::ffi_manager();
        graphics::lock!(graphics_manager, {
            unsafe {
                lvgl::lv_label_set_text_static(self.network, symbol.as_ptr());
            }
        });

        Ok(())
    }

    async fn update_clock(&mut self, current_time: Duration) {
        let graphics_manager = graphics::ffi_manager();
        graphics::lock!(graphics_manager, {
            self.clock_string.clear();
            // Writing to a String cannot return a formatting error.
            let _ = internationalization::write_unix_timestamp(
                &mut self.clock_string,
                current_time.as_secs() as i64,
                "%H:%M",
            );
            self.clock_string.push('\0');

            unsafe {
                lvgl::lv_label_set_text_static(
                    self.clock,
                    self.clock_string.as_ptr() as *const core::ffi::c_char,
                );
            }
        });
    }

    pub fn get_windows_parent(&self) -> *mut lvgl::lv_obj_t {
        self.window
    }

    pub async fn new(
        context: &'static xila::executable::ExecutableContext,
        show_keyboard: bool,
    ) -> Result<Self> {
        let graphics_manager = graphics::ffi_manager();
        let layout = graphics::lock!(graphics_manager, {
            // - Create a window
            let window = unsafe {
                let window = lvgl::lv_screen_active();

                if window.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_obj_set_size(window, lvgl::lv_pct(100), lvgl::lv_pct(100));
                lvgl::lv_obj_set_flex_flow(window, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);

                window
            };

            // - Create a header
            let header = unsafe {
                let header = lvgl::lv_obj_create(window);

                if header.is_null() {
                    return Err(Error::FailedToCreateObject);
                }
                lvgl::lv_obj_set_size(header, lvgl::lv_pct(100), lvgl::LV_SIZE_CONTENT);
                // Flat status-bar chrome: keep the theme surface but drop the
                // card padding, border and radius so the height stays slim.
                lvgl::lv_obj_set_style_pad_ver(header, 4, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_pad_hor(header, 8, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_border_width(header, 0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_radius(header, 0, lvgl::LV_STATE_DEFAULT);
                theme::set_surface(header, theme::Surface::Muted);

                header
            };

            // - - Create a label for the clock
            let clock = unsafe {
                let clock = lvgl::lv_label_create(header);

                if clock.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_obj_set_align(clock, lvgl::lv_align_t_LV_ALIGN_CENTER);

                clock
            };

            // - - Create a flex tray for the right side
            let tray = {
                unsafe {
                    let tray = lvgl::lv_obj_create(header);

                    if tray.is_null() {
                        return Err(Error::FailedToCreateObject);
                    }

                    lvgl::lv_obj_set_size(tray, lvgl::LV_SIZE_CONTENT, lvgl::LV_SIZE_CONTENT);
                    lvgl::lv_obj_set_flex_flow(tray, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
                    lvgl::lv_obj_set_style_pad_all(tray, 0, lvgl::LV_STATE_DEFAULT);
                    lvgl::lv_obj_set_style_border_width(tray, 0, lvgl::LV_STATE_DEFAULT);
                    lvgl::lv_obj_set_style_radius(tray, 0, lvgl::LV_STATE_DEFAULT);
                    lvgl::lv_obj_align(tray, lvgl::lv_align_t_LV_ALIGN_RIGHT_MID, 0, 0);
                    lvgl::lv_obj_set_style_bg_opa(
                        tray,
                        lvgl::LV_OPA_TRANSP as _,
                        lvgl::LV_STATE_DEFAULT,
                    );

                    tray
                }
            };

            // - - Create a label for the WiFi

            let network = unsafe {
                // - - Create a label for the WiFi

                let network = lvgl::lv_label_create(tray);

                if network.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_label_set_text(network, c"".as_ptr());

                network
            };

            // - - Create a label for the battery
            let battery = unsafe {
                // - - Create a label for the battery
                let battery = lvgl::lv_label_create(tray);

                if battery.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_label_set_text_static(battery, symbol::BATTERY_3.as_ptr());

                battery
            };

            // - - Create a body object
            let body = unsafe {
                // - Create a body
                let body = lvgl::lv_obj_create(window);

                if body.is_null() {
                    return Err(Error::FailedToCreateObject);
                }
                lvgl::lv_obj_add_flag(body, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
                lvgl::lv_obj_set_width(body, lvgl::lv_pct(100));
                lvgl::lv_obj_set_flex_grow(body, 1);
                // Edge-to-edge window area: the theme container would inset every
                // child window with card padding, border and radius.
                lvgl::lv_obj_set_style_pad_all(body, 0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_border_width(body, 0, lvgl::LV_STATE_DEFAULT);
                lvgl::lv_obj_set_style_radius(body, 0, lvgl::LV_STATE_DEFAULT);

                body
            };

            // - Create a keyboard
            unsafe {
                let keyboard = lvgl::lv_keyboard_create(window);

                if keyboard.is_null() {
                    return Err(Error::FailedToCreateObject);
                }

                lvgl::lv_obj_add_flag(keyboard, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);

                if show_keyboard {
                    lvgl::lv_obj_add_event_cb(
                        window,
                        Some(screen_event_handler),
                        EventKind::Focused.into_lvgl_code(),
                        keyboard as *mut _,
                    );
                    lvgl::lv_obj_add_event_cb(
                        window,
                        Some(screen_event_handler),
                        EventKind::Defocused.into_lvgl_code(),
                        keyboard as *mut _,
                    );
                    lvgl::lv_obj_add_event_cb(
                        keyboard,
                        Some(keyboard_event_handler),
                        EventKind::Ready.into_lvgl_code(),
                        keyboard as *mut _,
                    );
                    lvgl::lv_obj_add_event_cb(
                        window,
                        Some(keyboard_event_handler),
                        EventKind::Cancel.into_lvgl_code(),
                        keyboard as *mut _,
                    );
                }
            };

            Self {
                context,
                window,
                _header: header,
                _body: body,
                clock,
                clock_string: String::with_capacity(6),
                _battery: battery,
                network,
                last_update: Duration::ZERO,
            }
        });

        graphics_manager.set_window_parent(layout._body).await?;

        Ok(layout)
    }
}
