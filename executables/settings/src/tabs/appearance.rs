use crate::error::{Error, Result};
use alloc::ffi::CString;
use core::{ffi::CStr, ptr::null_mut};
use xila::graphics::appearance::Appearance;
use xila::{
    executable::ExecutableContext,
    graphics::{self, Event, EventKind, lvgl},
    internationalization::translate,
};

pub struct AppearanceTab {
    context: &'static ExecutableContext,
    appearance: Appearance,
    load_failed: bool,
    dark_mode: *mut lvgl::lv_obj_t,
    primary: *mut lvgl::lv_obj_t,
    secondary: *mut lvgl::lv_obj_t,
    status: *mut lvgl::lv_obj_t,
}

impl AppearanceTab {
    pub async fn new(context: &'static ExecutableContext) -> Self {
        let task = context.task_manager.get_current_task_identifier().await;
        let loaded = Appearance::load(context.virtual_file_system, task).await;
        if let Err(error) = &loaded {
            xila::log::warning!("Could not read appearance preferences: {}", error);
        }
        Self {
            context,
            load_failed: loaded.is_err(),
            appearance: loaded.unwrap_or_default(),
            dark_mode: null_mut(),
            primary: null_mut(),
            secondary: null_mut(),
            status: null_mut(),
        }
    }

    pub async fn create_ui(&mut self, parent: *mut lvgl::lv_obj_t) -> Result<*mut lvgl::lv_obj_t> {
        unsafe {
            let tab = check(lvgl::lv_tabview_add_tab(
                parent,
                translate!(c"tabs.appearance").as_ptr(),
            ))?;
            lvgl::lv_obj_set_flex_flow(tab, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_style_pad_all(tab, 16, 0);
            lvgl::lv_obj_set_style_pad_row(tab, 12, 0);

            let row = check(lvgl::lv_obj_create(tab))?;
            lvgl::lv_obj_set_width(row, lvgl::lv_pct(100));
            lvgl::lv_obj_set_height(row, lvgl::LV_SIZE_CONTENT);
            lvgl::lv_obj_set_flex_flow(row, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_ROW);
            lvgl::lv_obj_set_flex_align(
                row,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_START,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
            );
            graphics::theme::set_surface(row, graphics::theme::Surface::Card);
            let label = create_label(row, translate!(c"appearance.fields.dark_mode"))?;
            lvgl::lv_obj_set_flex_grow(label, 1);
            self.dark_mode = check(lvgl::lv_switch_create(row))?;

            // LVGL copies this options string; no per-preset objects are needed.
            let options = CString::new(alloc::format!(
                "{}\n{}\n{}\n{}\n{}\n{}",
                translate!("appearance.colors.zinc_auto"),
                translate!("appearance.colors.blue"),
                translate!("appearance.colors.green"),
                translate!("appearance.colors.orange"),
                translate!("appearance.colors.rose"),
                translate!("appearance.colors.violet")
            ))?;
            create_label(tab, translate!(c"appearance.fields.primary_color"))?;
            self.primary = check(lvgl::lv_dropdown_create(tab))?;
            lvgl::lv_dropdown_set_options(self.primary, options.as_ptr());
            lvgl::lv_obj_set_width(self.primary, lvgl::lv_pct(100));
            create_label(tab, translate!(c"appearance.fields.secondary_color"))?;
            self.secondary = check(lvgl::lv_dropdown_create(tab))?;
            lvgl::lv_dropdown_set_options(self.secondary, options.as_ptr());
            lvgl::lv_obj_set_width(self.secondary, lvgl::lv_pct(100));
            self.sync_controls();
            self.status = create_label(tab, c"")?;
            lvgl::lv_obj_set_width(self.status, lvgl::lv_pct(100));
            lvgl::lv_label_set_long_mode(
                self.status,
                lvgl::lv_label_long_mode_t_LV_LABEL_LONG_MODE_WRAP,
            );
            if self.load_failed {
                self.set_status(
                    translate!(
                        c"errors.could_not_load_appearance_choose_settings_to_save_a_replacement"
                    ),
                    true,
                );
            }
            Ok(tab)
        }
    }

    /// Settings dispatches events while holding the LVGL lock.
    pub async fn handle_event(&mut self, event: &Event) -> bool {
        if event.code != EventKind::ValueChanged
            || ![self.dark_mode, self.primary, self.secondary].contains(&event.target)
        {
            return false;
        }
        let appearance = unsafe {
            Appearance {
                is_dark: lvgl::lv_obj_has_state(self.dark_mode, lvgl::LV_STATE_CHECKED),
                primary: lvgl::lv_dropdown_get_selected(self.primary) as u8,
                secondary: lvgl::lv_dropdown_get_selected(self.secondary) as u8,
            }
        };
        if let Err(error) = appearance.apply(graphics::ffi_manager()).await {
            xila::log::warning!("Could not apply appearance: {}", error);
            unsafe {
                self.sync_controls();
                self.set_status(translate!(c"errors.could_not_apply_appearance"), true);
            }
            return true;
        }
        self.appearance = appearance;
        let task = self
            .context
            .task_manager
            .get_current_task_identifier()
            .await;
        match appearance
            .save(self.context.virtual_file_system, task)
            .await
        {
            Ok(()) => unsafe {
                self.set_status(translate!(c"appearance.status.saved"), false);
            },
            Err(error) => {
                xila::log::warning!("Could not save appearance preferences: {}", error);
                unsafe {
                    self.set_status(
                        translate!(c"appearance.status.applied_not_saved"),
                        true,
                    );
                }
            }
        }
        true
    }

    unsafe fn sync_controls(&self) {
        unsafe {
            lvgl::lv_obj_set_state(
                self.dark_mode,
                lvgl::LV_STATE_CHECKED,
                self.appearance.is_dark,
            );
            lvgl::lv_dropdown_set_selected(self.primary, u32::from(self.appearance.primary));
            lvgl::lv_dropdown_set_selected(self.secondary, u32::from(self.appearance.secondary));
        }
    }

    unsafe fn set_status(&self, message: &CStr, error: bool) {
        unsafe {
            lvgl::lv_label_set_text(self.status, message.as_ptr());
            lvgl::lv_theme_shadcn_set_error(self.status, error);
        }
    }
}

fn check(object: *mut lvgl::lv_obj_t) -> Result<*mut lvgl::lv_obj_t> {
    if object.is_null() {
        Err(Error::FailedToCreateUiElement)
    } else {
        Ok(object)
    }
}

unsafe fn create_label(parent: *mut lvgl::lv_obj_t, text: &CStr) -> Result<*mut lvgl::lv_obj_t> {
    unsafe {
        let label = check(lvgl::lv_label_create(parent))?;
        lvgl::lv_label_set_text(label, text.as_ptr());
        Ok(label)
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) mod tests {
    use super::*;
    use xila::graphics::Color;
    use xila::graphics::appearance::Appearance;

    pub async fn exercise_ui(
        context: &'static xila::executable::ExecutableContext,
        graphics: &'static xila::graphics::Manager,
    ) {
        let _lock = graphics.lock().await;
        unsafe {
            let display = lvgl::lv_display_get_default();
            let saved = *lvgl::lv_theme_shadcn_get_palette(display);
            assert!(!saved.dark);
            let expected = Appearance {
                is_dark: false,
                primary: 2,
                secondary: 5,
            }
            .colors();
            assert_eq!(Color::from(saved.primary), expected.0);
            assert_eq!(Color::from(saved.secondary), expected.1);

            // Keep assertion unwinding from dropping a window while this test
            // holds the LVGL lock. Successful runs clean up after unlocking.
            let mut window = core::mem::ManuallyDrop::new(graphics.create_window().await.unwrap());
            let mut other = core::mem::ManuallyDrop::new(graphics.create_window().await.unwrap());
            let button = lvgl::lv_button_create(other.as_object_mutable());
            let tabview = lvgl::lv_tabview_create(window.as_object_mutable());
            let mut tab = AppearanceTab::new(context).await;
            let container = tab.create_ui(tabview).await.unwrap();
            assert!(!container.is_null());
            assert_eq!(lvgl::lv_tabview_get_tab_count(tabview), 1);

            while window.pop_event().is_some() {}
            lvgl::lv_obj_send_event(
                tab.dark_mode,
                lvgl::lv_event_code_t_LV_EVENT_VALUE_CHANGED,
                core::ptr::null_mut(),
            );
            let bubbled = window.pop_event().unwrap();
            assert_eq!(bubbled.code, EventKind::ValueChanged);
            assert_eq!(bubbled.target, tab.dark_mode);

            assert_eq!(lvgl::lv_dropdown_get_selected(tab.primary), 2);
            assert_eq!(lvgl::lv_dropdown_get_selected(tab.secondary), 5);
            assert!(!lvgl::lv_obj_has_state(
                tab.dark_mode,
                lvgl::LV_STATE_CHECKED
            ));
            assert!(
                !tab.handle_event(&Event::new(EventKind::Clicked, tab.dark_mode, None))
                    .await
            );

            lvgl::lv_obj_add_state(tab.dark_mode, lvgl::LV_STATE_CHECKED);
            assert!(
                tab.handle_event(&Event::new(EventKind::ValueChanged, tab.dark_mode, None))
                    .await
            );
            let palette = *lvgl::lv_theme_shadcn_get_palette(display);
            assert!(palette.dark);
            assert_eq!(Color::from(palette.primary), expected.0);
            assert_eq!(Color::from(palette.secondary), expected.1);

            for (control, selected) in [(tab.primary, 1), (tab.secondary, 3)] {
                lvgl::lv_dropdown_set_selected(control, selected);
                assert!(
                    tab.handle_event(&Event::new(EventKind::ValueChanged, control, None))
                        .await
                );
            }
            let palette = *lvgl::lv_theme_shadcn_get_palette(display);
            let expected = Appearance {
                is_dark: true,
                primary: 1,
                secondary: 3,
            };
            let colors = expected.colors();
            assert_eq!(Color::from(palette.primary), colors.0);
            assert_eq!(Color::from(palette.secondary), colors.1);
            assert_eq!(Color::from(palette.ring), colors.0);
            assert_eq!(
                Color::from(lvgl::lv_obj_get_style_bg_color(button, 0)),
                colors.0
            );
            assert_eq!(
                Color::from(lvgl::lv_obj_get_style_text_color(button, 0)),
                Color::WHITE
            );
            let task = context.task_manager.get_current_task_identifier().await;
            assert_eq!(
                Appearance::load(context.virtual_file_system, task)
                    .await
                    .unwrap(),
                expected
            );

            let mut reopened = AppearanceTab::new(context).await;
            reopened.create_ui(tabview).await.unwrap();
            assert_eq!(lvgl::lv_dropdown_get_selected(reopened.primary), 1);
            assert_eq!(lvgl::lv_dropdown_get_selected(reopened.secondary), 3);
            assert!(lvgl::lv_obj_has_state(
                reopened.dark_mode,
                lvgl::LV_STATE_CHECKED
            ));
            // OwnedWindow::drop acquires the graphics lock itself.
            drop(_lock);
            core::mem::ManuallyDrop::drop(&mut other);
            core::mem::ManuallyDrop::drop(&mut window);
        }
    }
}
