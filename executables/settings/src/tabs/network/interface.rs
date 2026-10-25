use crate::{Error, Result, tabs::network::open_interface};
use alloc::string::String;
use core::fmt::Write;
use core::ptr::null_mut;
use xila::{
    graphics::{EventKind, lvgl},
    internationalization::translate,
    log,
    network::{
        GET_DNS_SERVER, GET_DNS_SERVER_COUNT, GET_HARDWARE_ADDRESS, GET_IP_ADDRESS,
        GET_IP_ADDRESS_COUNT, GET_ROUTE, GET_ROUTE_COUNT, MacAddress,
    },
    virtual_file_system::File,
};

#[derive(Default)]
struct IpConfigurationTab {
    pub _container: *mut lvgl::lv_obj_t,
    pub _radio_group: *mut lvgl::lv_obj_t,
    pub _radio_none: *mut lvgl::lv_obj_t,
    pub _radio_dhcp: *mut lvgl::lv_obj_t,
    pub _radio_static: *mut lvgl::lv_obj_t,
    pub _address_input: *mut lvgl::lv_obj_t,
    pub _gateway_input: *mut lvgl::lv_obj_t,
    pub _dns_inputs: [*mut lvgl::lv_obj_t; 3],
}

struct GeneralTab {
    pub _list: *mut lvgl::lv_obj_t,
}

pub struct InterfacePanel {
    main_container: *mut lvgl::lv_obj_t,

    cancel_button: *mut lvgl::lv_obj_t,
    apply_button: *mut lvgl::lv_obj_t,
    interface: String,
}

impl InterfacePanel {
    async fn create_general_tab(
        parent: *mut lvgl::lv_obj_t,
        file: &mut File<'_>,
    ) -> Result<GeneralTab> {
        unsafe {
            let list = lvgl::lv_list_create(parent);

            let mut format_buffer = String::with_capacity(64);

            {
                let hardware_address = Self::get_mac_address(file).await?;

                lvgl::lv_obj_set_size(list, lvgl::lv_pct(100), lvgl::lv_pct(100));

                write!(
                    format_buffer,
                    "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\0",
                    hardware_address[0],
                    hardware_address[1],
                    hardware_address[2],
                    hardware_address[3],
                    hardware_address[4],
                    hardware_address[5]
                )
                .ok();
                lvgl::lv_list_add_text(list, translate!(c"network.fields.mac_address").as_ptr());

                lvgl::lv_list_add_button(list, null_mut(), format_buffer.as_ptr() as _);
            }

            {
                let ip_count = file
                    .as_synchronous_file_mut()
                    .control(GET_IP_ADDRESS_COUNT, &())?;

                lvgl::lv_list_add_text(list, translate!(c"network.fields.address").as_ptr());

                for index in 0..ip_count {
                    let ip = file
                        .as_synchronous_file_mut()
                        .control(GET_IP_ADDRESS, &index)?;
                    format_buffer.clear();
                    write!(format_buffer, "{}\0", ip).ok();

                    lvgl::lv_list_add_button(list, null_mut(), format_buffer.as_ptr() as _);
                }
            }

            {
                let route_count = file
                    .as_synchronous_file_mut()
                    .control(GET_ROUTE_COUNT, &())?;

                lvgl::lv_list_add_text(list, translate!(c"network.fields.routes").as_ptr());

                for index in 0..route_count {
                    let route = file.as_synchronous_file_mut().control(GET_ROUTE, &index)?;
                    format_buffer.clear();
                    write!(format_buffer, "{} via {}\0", route.cidr, route.via_router).ok();
                    lvgl::lv_list_add_button(list, null_mut(), format_buffer.as_ptr() as _);
                }
            }

            {
                let dns_count = file
                    .as_synchronous_file_mut()
                    .control(GET_DNS_SERVER_COUNT, &())?;

                lvgl::lv_list_add_text(list, translate!(c"network.fields.dns_servers").as_ptr());

                for index in 0..dns_count {
                    let dns = file
                        .as_synchronous_file_mut()
                        .control(GET_DNS_SERVER, &index)?;
                    format_buffer.clear();
                    write!(format_buffer, "{}\0", dns).ok();
                    lvgl::lv_list_add_button(list, null_mut(), format_buffer.as_ptr() as _);
                }
            }

            Ok(GeneralTab { _list: list })
        }
    }

    fn create_ip_configuration_tab(
        parent: *mut lvgl::lv_obj_t,
        is_ipv6: bool,
        tab_name: &[u8],
    ) -> Result<IpConfigurationTab> {
        unsafe {
            // Create tab for this IP version
            let container = lvgl::lv_tabview_add_tab(parent, tab_name.as_ptr() as *const _);
            if container.is_null() {
                return Err(Error::FailedToCreateObject);
            }

            lvgl::lv_obj_set_size(container, lvgl::lv_pct(100), lvgl::lv_pct(100));
            lvgl::lv_obj_set_flex_flow(container, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_flex_align(
                container,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_START,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_START,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
            );

            // Label: Configuration Mode
            let mode_label = lvgl::lv_label_create(container);
            let mode_text = translate!(c"network.fields.configuration_mode");
            lvgl::lv_label_set_text(mode_label, mode_text.as_ptr() as *const _);

            // Radio group for configuration modew
            let radio_group = lvgl::lv_obj_create(container);
            lvgl::lv_obj_set_size(radio_group, lvgl::LV_SIZE_CONTENT, lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_flow(radio_group, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);

            // Radio: None
            let radio_none = lvgl::lv_radiobox_create(radio_group);
            let none_text = translate!(c"common.none");
            lvgl::lv_checkbox_set_text(radio_none, none_text.as_ptr() as *const _);

            // Radio: DHCP
            let radio_dhcp = lvgl::lv_radiobox_create(radio_group);
            let dhcp_text = translate!(c"network.modes.dhcp");
            lvgl::lv_checkbox_set_text(radio_dhcp, dhcp_text.as_ptr() as *const _);

            // Radio: Static
            let radio_static = lvgl::lv_radiobox_create(radio_group);
            let static_text = translate!(c"network.modes.static");
            lvgl::lv_checkbox_set_text(radio_static, static_text.as_ptr() as *const _);

            // Container for static configuration (hidden by default)

            // IP Address + CIDR input
            let address_label = lvgl::lv_label_create(container);
            let address_text = translate!(c"network.fields.address");
            lvgl::lv_label_set_text(address_label, address_text.as_ptr() as *const _);

            let address_input = lvgl::lv_textarea_create(container);
            if is_ipv6 {
                lvgl::lv_textarea_set_placeholder_text(
                    address_input,
                    c"2001:0db8:85a3::8a2e:0370:7334/64".as_ptr(),
                );
            } else {
                lvgl::lv_textarea_set_placeholder_text(address_input, c"192.168.1.100/24".as_ptr());
            }
            lvgl::lv_textarea_set_one_line(address_input, true);
            lvgl::lv_obj_set_width(address_input, lvgl::lv_pct(100));

            // Gateway input
            let gateway_label = lvgl::lv_label_create(container);
            let gateway_text = translate!(c"network.fields.gateway");
            lvgl::lv_label_set_text(gateway_label, gateway_text.as_ptr() as *const _);

            let gateway_input = lvgl::lv_textarea_create(container);
            if is_ipv6 {
                lvgl::lv_textarea_set_placeholder_text(gateway_input, c"fe80::1".as_ptr());
            } else {
                lvgl::lv_textarea_set_placeholder_text(gateway_input, c"192.168.1.1".as_ptr());
            }
            lvgl::lv_textarea_set_one_line(gateway_input, true);
            lvgl::lv_obj_set_width(gateway_input, lvgl::lv_pct(100));

            // DNS inputs
            let dns_label = lvgl::lv_label_create(container);
            let dns_text = translate!(c"network.fields.dns_servers");
            lvgl::lv_label_set_text(dns_label, dns_text.as_ptr() as *const _);

            let mut dns_inputs = [null_mut(); 3];

            for (i, dns_input) in dns_inputs.iter_mut().enumerate() {
                *dns_input = lvgl::lv_textarea_create(container);

                if is_ipv6 {
                    lvgl::lv_textarea_set_placeholder_text(
                        *dns_input,
                        if i == 0 {
                            c"2606:4700:4700::1111".as_ptr()
                        } else if i == 1 {
                            c"2001:4860:4860::8888".as_ptr()
                        } else {
                            c"2001:4860:4860::8844".as_ptr()
                        },
                    );
                } else {
                    lvgl::lv_textarea_set_placeholder_text(
                        *dns_input,
                        if i == 0 {
                            c"1.1.1.1".as_ptr()
                        } else if i == 1 {
                            c"8.8.8.8".as_ptr()
                        } else {
                            c"8.8.4.4".as_ptr()
                        },
                    );
                }

                lvgl::lv_textarea_set_one_line(*dns_input, true);
                lvgl::lv_obj_set_width(*dns_input, lvgl::lv_pct(100));
            }

            // TODO: Connect radio buttons to show/hide static_container
            // TODO: Implement radio button group behavior (only one can be selected)

            Ok(IpConfigurationTab {
                _container: container,
                _radio_group: radio_group,
                _radio_none: radio_none,
                _radio_dhcp: radio_dhcp,
                _radio_static: radio_static,
                _address_input: address_input,
                _gateway_input: gateway_input,
                _dns_inputs: dns_inputs,
            })
        }
    }

    async fn get_mac_address(file: &mut File<'_>) -> Result<MacAddress> {
        Ok(file.control(GET_HARDWARE_ADDRESS, &()).await?)
    }

    pub async fn new(
        context: &'static xila::executable::ExecutableContext,
        interface: String,
        parent_tabview: *mut lvgl::lv_obj_t,
    ) -> Result<Self> {
        // Create a container for the entire configuration panel
        let main_container = unsafe { lvgl::lv_obj_create(parent_tabview) };
        if main_container.is_null() {
            return Err(Error::FailedToCreateObject);
        }

        unsafe {
            lvgl::lv_obj_add_flag(main_container, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_FLOATING);
            lvgl::lv_obj_set_size(main_container, lvgl::lv_pct(100), lvgl::lv_pct(100));
            lvgl::lv_obj_set_flex_flow(main_container, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_style_pad_all(main_container, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_border_width(main_container, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_style_radius(main_container, 0, lvgl::LV_STATE_DEFAULT);
        }

        // Create button container
        let button_container = unsafe { lvgl::lv_obj_create(main_container) };
        if button_container.is_null() {
            return Err(Error::FailedToCreateObject);
        }

        unsafe {
            lvgl::lv_obj_set_size(button_container, lvgl::lv_pct(100), lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_flow(button_container, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
            lvgl::lv_obj_set_flex_align(
                button_container,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_SPACE_BETWEEN,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
            );
            lvgl::lv_obj_set_style_border_side(
                button_container,
                lvgl::lv_border_side_t_LV_BORDER_SIDE_BOTTOM,
                lvgl::LV_PART_MAIN,
            );
        }

        let cancel_button = unsafe {
            // Cancel button
            let cancel_button = lvgl::lv_button_create(button_container);
            let cancel_label = lvgl::lv_label_create(cancel_button);
            let cancel_text = translate!(c"actions.cancel");
            lvgl::lv_label_set_text(cancel_label, cancel_text.as_ptr() as *const _);

            cancel_button
        };

        // Title
        unsafe {
            let title_label = lvgl::lv_label_create(button_container);
            let title_text = c"Interface Configuration";
            lvgl::lv_label_set_text(title_label, title_text.as_ptr() as *const _);
        }

        // Apply button
        let apply_button = unsafe {
            let apply_button = lvgl::lv_button_create(button_container);
            let apply_label = lvgl::lv_label_create(apply_button);
            let apply_text = translate!(c"actions.apply");
            lvgl::lv_label_set_text(apply_label, apply_text.as_ptr() as *const _);
            apply_button
        };

        // Create a tabview for IPv4 and IPv6 configurations
        let config_tabview = unsafe { lvgl::lv_tabview_create(main_container) };
        if config_tabview.is_null() {
            return Err(Error::FailedToCreateObject);
        }

        unsafe {
            lvgl::lv_obj_set_width(config_tabview, lvgl::lv_pct(100));
            lvgl::lv_obj_set_flex_grow(config_tabview, 1);
        }

        // Create general tab
        let general_tab = unsafe {
            let tab = lvgl::lv_tabview_add_tab(
                config_tabview,
                translate!(c"tabs.general").as_ptr() as *const _,
            );
            if tab.is_null() {
                return Err(Error::FailedToCreateObject);
            }
            tab
        };

        // Create general tab content
        let virtual_file_system = &context.virtual_file_system;

        let mut file = open_interface(virtual_file_system, &interface).await?;

        let _general_tab = Self::create_general_tab(general_tab, &mut file).await?;

        file.close(virtual_file_system).await?;

        // Create IPv4 tab
        let _ipv4_tab = Self::create_ip_configuration_tab(config_tabview, false, b"IPv4\0")?;

        // Create IPv6 tab
        let _ipv6_tab = Self::create_ip_configuration_tab(config_tabview, true, b"IPv6\0")?;

        let interface = Self {
            main_container,
            cancel_button,
            apply_button,
            interface,
        };

        Ok(interface)
    }

    pub async fn handle_event(&mut self, event: &xila::graphics::Event) -> bool {
        // Allow only selection of one radio button at a time

        if event.code == EventKind::Clicked {
            if event.target == self.cancel_button {
                log::information!("Cancel button clicked in interface panel");
                // Close the panel without applying changes
                return false;
            } else if event.target == self.apply_button {
                log::information!("Apply button clicked in interface panel");
                // Apply the configuration changes
                return false;
            }
        }

        true
    }
}

impl Drop for InterfacePanel {
    fn drop(&mut self) {
        unsafe {
            lvgl::lv_obj_delete(self.main_container);
            log::information!("Interface panel for {} has been deleted", self.interface);
        }
    }
}
