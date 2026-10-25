//! Wi-Fi UI owns no saved profiles. All actions use bounded VFS controls.
use crate::{Error, Result};
use alloc::{ffi::CString, string::String};
use core::{ffi::CStr, fmt::Write, ptr::null_mut};
use xila::{
    file_system::ControlCommand,
    graphics::{Event, EventKind, lvgl},
    internationalization::translate,
    network::*,
    virtual_file_system::File,
};

pub(super) struct WifiPanel {
    file: File<'static>,
    capabilities: WifiCapabilities,
    radio: *mut lvgl::lv_obj_t,
    scan: *mut lvgl::lv_obj_t,
    networks: *mut lvgl::lv_obj_t,
    password: *mut lvgl::lv_obj_t,
    connect: *mut lvgl::lv_obj_t,
    disconnect: *mut lvgl::lv_obj_t,
    status: *mut lvgl::lv_obj_t,
    details: *mut lvgl::lv_obj_t,
    generation: Option<u32>,
    count: u16,
    message: Option<&'static CStr>,
}
impl WifiPanel {
    pub fn new(
        parent: *mut lvgl::lv_obj_t,
        file: File<'static>,
        capabilities: WifiCapabilities,
    ) -> Result<Self> {
        let mut panel = Self {
            file,
            capabilities,
            radio: null_mut(),
            scan: null_mut(),
            networks: null_mut(),
            password: null_mut(),
            connect: null_mut(),
            disconnect: null_mut(),
            status: null_mut(),
            details: null_mut(),
            generation: None,
            count: 0,
            message: None,
        };
        unsafe {
            let tab = check(lvgl::lv_tabview_add_tab(
                parent,
                translate!(c"wifi.title").as_ptr(),
            ))?;
            lvgl::lv_obj_set_flex_flow(tab, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_style_pad_row(tab, 10, 0);
            let row = check(lvgl::lv_obj_create(tab))?;
            lvgl::lv_obj_set_size(row, lvgl::lv_pct(100), lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_flow(row, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
            let caption = label(row, translate!(c"wifi.radio"))?;
            lvgl::lv_obj_set_width(caption, lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_grow(caption, 1);
            panel.radio = check(lvgl::lv_switch_create(row))?;
            panel.scan = button(row, translate!(c"wifi.scan"))?;
            panel.status = label(tab, translate!(c"wifi.disconnected"))?;
            panel.details = label(tab, c"")?;
            label(tab, translate!(c"wifi.networks"))?;
            panel.networks = check(lvgl::lv_dropdown_create(tab))?;
            lvgl::lv_obj_set_width(panel.networks, lvgl::lv_pct(100));
            lvgl::lv_dropdown_set_options(panel.networks, translate!(c"wifi.no_networks").as_ptr());
            label(tab, translate!(c"password.fields.password"))?;
            panel.password = check(lvgl::lv_textarea_create(tab))?;
            lvgl::lv_textarea_set_one_line(panel.password, true);
            lvgl::lv_textarea_set_password_mode(panel.password, true);
            lvgl::lv_textarea_set_password_show_time(panel.password, 0);
            lvgl::lv_textarea_set_max_length(panel.password, 64);
            lvgl::lv_obj_set_width(panel.password, lvgl::lv_pct(100));
            let actions = check(lvgl::lv_obj_create(tab))?;
            lvgl::lv_obj_set_size(actions, lvgl::lv_pct(100), lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_flow(actions, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
            panel.connect = button(actions, translate!(c"wifi.connect"))?;
            panel.disconnect = button(actions, translate!(c"wifi.disconnect"))?;
        }
        panel.refresh();
        Ok(panel)
    }
    fn control<C: ControlCommand>(
        &mut self,
        command: C,
        input: &C::Input,
    ) -> xila::virtual_file_system::Result<C::Output>
    where
        C::Output: Default,
    {
        // One probe: a busy operation must never suspend the Settings graphics lock.
        self.file.as_synchronous_file_mut().control(command, input)
    }
    fn selected(&mut self) -> xila::virtual_file_system::Result<WifiAccessPoint> {
        let index = unsafe { lvgl::lv_dropdown_get_selected(self.networks) };
        self.control(
            GET_WIFI_SCAN_RESULT,
            &WifiScanResultRequest {
                generation: self.generation.unwrap_or(0),
                index: index as u16,
                reserved: [0; 2],
            },
        )
    }
    fn clear_password(&self) {
        if !self.password.is_null() {
            unsafe {
                lvgl::lv_textarea_set_text(self.password, c"".as_ptr());
            }
        }
    }
    pub fn refresh(&mut self) {
        if self.refresh_inner().is_err() {
            unsafe {
                lvgl::lv_label_set_text(self.status, translate!(c"wifi.unavailable").as_ptr());
            }
        } else if self.message == Some(translate!(c"wifi.busy")) {
            // Render a rejected action once, then let the next timer probe
            // show live progress without requiring another user action.
            self.message = None;
        }
    }
    fn refresh_inner(&mut self) -> xila::virtual_file_system::Result<()> {
        let radio = self.control(GET_WIFI_RADIO_STATE, &())?;
        let scan = self.control(GET_WIFI_SCAN_STATE, &())?;
        let connection = self.control(GET_WIFI_CONNECTION_STATE, &())?;
        let error = self.control(GET_WIFI_LAST_CONNECTION_ERROR, &())?;
        let busy = scan.phase == WifiScanPhase::SCANNING
            || connection == WifiConnectionState::CONNECTING
            || connection == WifiConnectionState::DISCONNECTING;
        let enabled = radio == WifiRadioState::ON;
        let count = self.control(GET_WIFI_SCAN_RESULT_COUNT, &())?;
        if self.generation != Some(count.generation) {
            let mut options = String::new();
            let limit = count
                .count
                .min(u16::from(self.capabilities.maximum_scan_results))
                .min(16);
            for index in 0..limit {
                let ap = self.control(
                    GET_WIFI_SCAN_RESULT,
                    &WifiScanResultRequest {
                        generation: count.generation,
                        index,
                        reserved: [0; 2],
                    },
                )?;
                if index > 0 {
                    options.push('\n');
                }
                let _ = write!(
                    options,
                    "{}  ({}, {} dBm)",
                    ssid_label(&ap.ssid),
                    authentication_label(ap.authentication).to_string_lossy(),
                    ap.rssi
                );
            }
            if options.is_empty() {
                options.push_str(translate!("wifi.no_networks"));
            }
            if let Ok(options) = CString::new(options) {
                unsafe {
                    lvgl::lv_dropdown_set_options(self.networks, options.as_ptr());
                }
            }
            self.clear_password();
            self.count = limit;
            self.generation = Some(count.generation);
        }
        let selected = if self.count > 0 {
            Some(self.selected()?)
        } else {
            None
        };
        let supported = selected.is_some_and(|ap| self.capabilities.supports(ap.authentication));
        let secured = selected.is_some_and(|ap| ap.authentication != WifiAuthentication::OPEN);
        let text = self.message.unwrap_or_else(|| {
            if !enabled {
                translate!(c"wifi.radio_off")
            } else if scan.phase == WifiScanPhase::SCANNING {
                translate!(c"wifi.scanning")
            } else if connection == WifiConnectionState::CONNECTING {
                translate!(c"wifi.connecting")
            } else if connection == WifiConnectionState::DISCONNECTING {
                translate!(c"wifi.disconnecting")
            } else if connection == WifiConnectionState::CONNECTED {
                translate!(c"wifi.connected")
            } else {
                error_label(error)
            }
        });
        unsafe {
            lvgl::lv_obj_set_state(self.radio, lvgl::LV_STATE_CHECKED, enabled);
            disabled(self.scan, !enabled || busy);
            disabled(self.networks, !enabled || busy || self.count == 0);
            disabled(self.password, !enabled || busy || !secured || !supported);
            disabled(self.connect, !enabled || busy || !supported);
            disabled(
                self.disconnect,
                !enabled || busy || connection != WifiConnectionState::CONNECTED,
            );
            lvgl::lv_label_set_text(self.status, text.as_ptr());
            let info = if connection == WifiConnectionState::CONNECTED {
                Some(self.control(GET_WIFI_CONNECTION_INFO, &())?)
            } else {
                selected
            };
            if let Some(ap) = info {
                let details = CString::new(alloc::format!(
                    "{} | {} dBm | {} {} | {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                    ssid_label(&ap.ssid),
                    ap.rssi,
                    translate!("wifi.channel"),
                    ap.channel,
                    ap.bssid[0],
                    ap.bssid[1],
                    ap.bssid[2],
                    ap.bssid[3],
                    ap.bssid[4],
                    ap.bssid[5]
                ))
                .unwrap();
                lvgl::lv_label_set_text(self.details, details.as_ptr());
            } else {
                lvgl::lv_label_set_text(self.details, c"".as_ptr());
            }
        }
        Ok(())
    }
    pub fn handle_event(&mut self, event: &Event) {
        let result = if event.target == self.radio && event.code == EventKind::ValueChanged {
            self.message = None;
            self.clear_password();
            let radio = if unsafe { lvgl::lv_obj_has_state(self.radio, lvgl::LV_STATE_CHECKED) } {
                WifiRadioState::ON
            } else {
                WifiRadioState::OFF
            };
            self.control(SET_WIFI_RADIO_STATE, &radio)
        } else if event.target == self.scan && event.code == EventKind::Clicked {
            self.message = None;
            self.clear_password();
            self.control(START_WIFI_SCAN, &WifiScanConfiguration::default())
        } else if event.target == self.disconnect && event.code == EventKind::Clicked {
            self.message = None;
            self.clear_password();
            self.control(DISCONNECT_WIFI_CLIENT, &())
        } else if event.target == self.connect && event.code == EventKind::Clicked {
            self.message = None;
            let result = self.connect_selected();
            self.clear_password();
            result
        } else if event.target == self.networks && event.code == EventKind::ValueChanged {
            self.message = None;
            self.clear_password();
            Ok(())
        } else {
            return;
        };
        if let Err(error) = result {
            self.message = Some(
                if matches!(
                    error,
                    xila::virtual_file_system::Error::RessourceBusy
                        | xila::virtual_file_system::Error::FileSystem(
                            xila::file_system::Error::RessourceBusy
                        )
                ) {
                    translate!(c"wifi.busy")
                } else {
                    translate!(c"wifi.command_failed")
                },
            );
        }
        self.refresh();
    }
    fn connect_selected(&mut self) -> xila::virtual_file_system::Result<()> {
        let ap = self.selected()?;
        if !self.capabilities.supports(ap.authentication) {
            return Err(xila::file_system::Error::UnsupportedOperation.into());
        }
        // Borrow widget bytes only for construction; the driver receives a bounded owned copy.
        let secret =
            unsafe { CStr::from_ptr(lvgl::lv_textarea_get_text(self.password)) }.to_bytes();
        let mut config = WifiClientConfiguration::new(
            ap.ssid.as_bytes()?,
            ap.authentication,
            if ap.authentication == WifiAuthentication::OPEN {
                &[]
            } else {
                secret
            },
        )?;
        config.use_bssid = 1;
        config.bssid = ap.bssid;
        config.channel = ap.channel;
        self.control(CONNECT_WIFI_CLIENT, &config)
    }
}
impl Drop for WifiPanel {
    fn drop(&mut self) {
        self.clear_password();
    }
}

fn ssid_label(ssid: &WifiSsid) -> String {
    let mut label = String::new();
    for &byte in ssid.as_bytes().unwrap_or_default() {
        if (32..=126).contains(&byte) {
            label.push(byte as char);
        } else {
            let _ = write!(label, "\\x{byte:02X}");
        }
    }
    label
}
fn authentication_label(authentication: WifiAuthentication) -> &'static CStr {
    match authentication {
        WifiAuthentication::OPEN => translate!(c"wifi.open"),
        WifiAuthentication::WPA2_PERSONAL => c"WPA2",
        WifiAuthentication::WPA3_PERSONAL => c"WPA3",
        WifiAuthentication::WPA2_WPA3_PERSONAL => c"WPA2/WPA3",
        _ => translate!(c"wifi.unsupported"),
    }
}
fn error_label(error: WifiConnectionErrorKind) -> &'static CStr {
    match error {
        WifiConnectionErrorKind::NONE => translate!(c"wifi.disconnected"),
        WifiConnectionErrorKind::AUTHENTICATION => translate!(c"wifi.authentication_failed"),
        WifiConnectionErrorKind::ACCESS_POINT_NOT_FOUND => translate!(c"wifi.not_found"),
        WifiConnectionErrorKind::TIMEOUT => translate!(c"wifi.timeout"),
        WifiConnectionErrorKind::UNSUPPORTED_CONFIGURATION => translate!(c"wifi.unsupported"),
        _ => translate!(c"wifi.connection_failed"),
    }
}
unsafe fn disabled(object: *mut lvgl::lv_obj_t, value: bool) {
    unsafe {
        lvgl::lv_obj_set_state(object, lvgl::lv_state_t_LV_STATE_DISABLED, value);
    }
}
fn check(object: *mut lvgl::lv_obj_t) -> Result<*mut lvgl::lv_obj_t> {
    if object.is_null() {
        Err(Error::FailedToCreateObject)
    } else {
        Ok(object)
    }
}
unsafe fn label(parent: *mut lvgl::lv_obj_t, text: &CStr) -> Result<*mut lvgl::lv_obj_t> {
    unsafe {
        let object = check(lvgl::lv_label_create(parent))?;
        lvgl::lv_obj_set_width(object, lvgl::lv_pct(100));
        lvgl::lv_label_set_text(object, text.as_ptr());
        Ok(object)
    }
}
unsafe fn button(parent: *mut lvgl::lv_obj_t, text: &CStr) -> Result<*mut lvgl::lv_obj_t> {
    unsafe {
        let object = check(lvgl::lv_button_create(parent))?;
        // Button labels size to text rather than the parent's entire width.
        let caption = check(lvgl::lv_label_create(object))?;
        lvgl::lv_label_set_text(caption, text.as_ptr());
        Ok(object)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub async fn exercise_ui(
        context: &'static xila::executable::ExecutableContext,
        graphics: &'static xila::graphics::Manager,
    ) {
        use alloc::boxed::Box;
        use core::time::Duration;
        use drivers_native::wifi::*;
        let task = context.task_manager.get_current_task_identifier().await;
        let manager = Box::leak(Box::new(xila::network::initialize(
            context.task_manager,
            context.virtual_file_system,
            context.time_manager,
            &drivers_shared::devices::RandomDevice,
        )));
        let mut fixture = MockWifiConfiguration::demo();
        fixture.scan_latency = Duration::from_millis(10);
        fixture.connect_latency = Duration::from_millis(10);
        let (controller, runner) = MockWifiController::new(fixture).unwrap();
        let (packets, _) = create_loopback_device();
        manager
            .mount_interface(task, "wlan0", packets, controller, None)
            .await
            .unwrap();
        context
            .task_manager
            .spawn(task, "Wi-Fi UI test", None, move |_| async move {
                runner.run().await
            })
            .await
            .unwrap();
        // The native runtime also mounts protocol endpoints beside interfaces.
        context
            .virtual_file_system
            .mount_character_device(
                task,
                "/devices/network/http_client",
                drivers_core::NullDevice,
            )
            .await
            .unwrap();
        let lock = graphics.lock().await;
        let mut window = core::mem::ManuallyDrop::new(graphics.create_window().await.unwrap());
        let tabview = unsafe { lvgl::lv_tabview_create(window.as_object_mutable()) };
        let mut network = super::super::NetworkTab::new(context);
        network.create_ui(tabview).await.unwrap();
        let file = super::super::open_interface(context.virtual_file_system, "wlan0")
            .await
            .unwrap();
        let capabilities = WifiCapabilities {
            station: 1,
            access_point: 0,
            authentication_mask: 15,
            maximum_scan_results: 16,
        };
        let mut panel = WifiPanel::new(tabview, file, capabilities).unwrap();
        panel.handle_event(&Event::new(EventKind::Clicked, panel.scan, None));
        assert_eq!(
            panel.control(GET_WIFI_SCAN_STATE, &()).unwrap().phase,
            WifiScanPhase::SCANNING
        );
        drop(lock);
        xila::task::sleep(Duration::from_millis(60)).await;
        let lock = graphics.lock().await;
        panel.refresh();
        assert_eq!(panel.count, 2);
        panel
            .control(START_WIFI_SCAN, &WifiScanConfiguration::default())
            .unwrap();
        // Another client can start work between a rendered button and its click.
        unsafe {
            lvgl::lv_textarea_set_text(panel.password, c"xila-demo".as_ptr());
        }
        panel.handle_event(&Event::new(EventKind::Clicked, panel.connect, None));
        assert_eq!(
            unsafe { CStr::from_ptr(lvgl::lv_label_get_text(panel.status)) },
            translate!(c"wifi.busy")
        );
        panel.refresh();
        assert_eq!(
            unsafe { CStr::from_ptr(lvgl::lv_label_get_text(panel.status)) },
            translate!(c"wifi.scanning")
        );
        drop(lock);
        xila::task::sleep(Duration::from_millis(60)).await;
        let lock = graphics.lock().await;
        panel.refresh();

        unsafe {
            lvgl::lv_textarea_set_text(panel.password, c"wrong-key".as_ptr());
        }
        panel.handle_event(&Event::new(EventKind::Clicked, panel.connect, None));
        assert!(unsafe { CStr::from_ptr(lvgl::lv_textarea_get_text(panel.password)) }.is_empty());
        drop(lock);
        xila::task::sleep(Duration::from_millis(60)).await;
        let lock = graphics.lock().await;
        panel.refresh();
        assert_eq!(
            panel.control(GET_WIFI_LAST_CONNECTION_ERROR, &()).unwrap(),
            WifiConnectionErrorKind::AUTHENTICATION
        );
        unsafe {
            lvgl::lv_textarea_set_text(panel.password, c"xila-demo".as_ptr());
        }
        panel.handle_event(&Event::new(EventKind::Clicked, panel.connect, None));
        drop(lock);
        xila::task::sleep(Duration::from_millis(60)).await;
        let lock = graphics.lock().await;
        panel.refresh();
        assert_eq!(
            panel.control(GET_WIFI_CONNECTION_STATE, &()).unwrap(),
            WifiConnectionState::CONNECTED
        );
        // Drop clears the widget before its parent tree is destroyed.
        let password = panel.password;
        drop(panel);
        assert!(unsafe { CStr::from_ptr(lvgl::lv_textarea_get_text(password)) }.is_empty());
        drop(lock);
        unsafe {
            core::mem::ManuallyDrop::drop(&mut window);
        }
    }

    #[test]
    fn ssid_labels_cannot_inject_dropdown_rows_or_nul() {
        let name = xila::network::WifiSsid::new(b"cafe\n\0\xff").unwrap();
        let text = ssid_label(&name);
        assert!(!text.contains('\n'));
        assert!(!text.contains('\0'));
        assert!(text.starts_with("cafe"));
    }
}
