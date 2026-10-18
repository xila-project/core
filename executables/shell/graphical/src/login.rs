use alloc::string::ToString;
pub(crate) use core::ffi::CStr;
use xila::authentication;
use xila::graphics::{self, EventKind, OwnedWindow, lvgl};
use xila::internationalization::translate;
use xila::users::UserIdentifier;

use crate::error::{Error, Result};

pub struct Login {
    context: &'static xila::executable::ExecutableContext,
    graphics_manager: &'static graphics::Manager,
    window: OwnedWindow,
    user_name_text_area: *mut lvgl::lv_obj_t,
    password_text_area: *mut lvgl::lv_obj_t,
    button: *mut lvgl::lv_obj_t,
    error_label: *mut lvgl::lv_obj_t,
    user: Option<UserIdentifier>,
}

impl Login {
    pub async fn new(context: &'static xila::executable::ExecutableContext) -> Result<Self> {
        let graphics_manager = graphics::ffi_manager();
        // - Lock the graphics
        graphics::lock!(graphics_manager, {
            // - Create a window
            let mut window = graphics_manager.create_window().await?;

            unsafe {
                lvgl::lv_obj_set_flex_flow(window.as_object_mutable(), lvgl::LV_FLEX_COLUMN);
                lvgl::lv_obj_set_flex_align(
                    window.as_object_mutable(),
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                    lvgl::lv_flex_align_t_LV_FLEX_ALIGN_CENTER,
                );
            }

            let user_name_text_area = unsafe {
                // - Create a text area for the user name
                let user_name_text_area = lvgl::lv_textarea_create(window.as_object_mutable());

                lvgl::lv_textarea_set_placeholder_text(
                    user_name_text_area,
                    translate!(c"User name").as_ptr(),
                );
                lvgl::lv_textarea_set_one_line(user_name_text_area, true);

                user_name_text_area
            };

            let password_text_area = unsafe {
                // - Create a text area for the password
                let password_text_area = lvgl::lv_textarea_create(window.as_mut());

                lvgl::lv_textarea_set_placeholder_text(
                    password_text_area,
                    translate!(c"Password").as_ptr(),
                );
                lvgl::lv_textarea_set_one_line(password_text_area, true);
                lvgl::lv_textarea_set_password_mode(password_text_area, true);

                password_text_area
            };

            let error_label = unsafe {
                // - Create a label for the error
                let error_label = lvgl::lv_label_create(window.as_mut());

                lvgl::lv_label_set_text(error_label, c"".as_ptr());

                error_label
            };

            let button = unsafe {
                // - Create a button
                let button = lvgl::lv_button_create(window.as_mut());

                let label = lvgl::lv_label_create(button);

                lvgl::lv_label_set_text(label, translate!(c"Login").as_ptr());

                button
            };

            Ok(Login {
                context,
                graphics_manager,
                window,
                user_name_text_area,
                password_text_area,
                button,
                error_label,
                user: None,
            })
        })
    }

    pub fn print_error(&mut self, error: Error) {
        let error = error.to_string();
        let error = error.as_bytes();

        unsafe {
            let error = CStr::from_bytes_with_nul_unchecked(error);

            lvgl::lv_label_set_text(self.error_label, error.as_ptr());
        }
    }

    pub fn clear_error(&mut self) {
        unsafe {
            lvgl::lv_label_set_text(self.error_label, c"".as_ptr());
        }
    }

    pub async fn authenticate(&mut self) -> Result<()> {
        let (user_name, password) = unsafe {
            let user_name = lvgl::lv_textarea_get_text(self.user_name_text_area);
            let user_name = CStr::from_ptr(user_name);

            let user_name = user_name.to_str().map_err(Error::InvalidUtf8)?;

            let password = lvgl::lv_textarea_get_text(self.password_text_area);
            let password = CStr::from_ptr(password);
            let password = password.to_str().map_err(Error::InvalidUtf8)?;

            (user_name, password)
        };

        // - Check the user name and the password
        let task_manager = &self.context.task_manager;
        let task = task_manager.get_current_task_identifier().await;
        let authentication_context = authentication::Context {
            virtual_file_system: self.context.virtual_file_system,
            task_manager,
            users_manager: self.context.users_manager,
            task,
        };
        let user_identifier =
            authentication::authenticate_user(&authentication_context, user_name, password)
                .await
                .map_err(Error::AuthenticationFailed)?;

        let home = self
            .context
            .virtual_file_system
            .ensure_user_home(task, user_identifier)
            .await
            .map_err(Error::FailedToOpenDirectory)?;
        task_manager
            .set_environment_variable(task, "HOME", home.as_str())
            .await
            .map_err(Error::FailedToSetEnvironmentVariable)?;

        // - Set the user
        task_manager
            .set_user(task, user_identifier)
            .await
            .map_err(Error::FailedToSetTaskUser)?;

        self.user = Some(user_identifier);

        Ok(())
    }

    pub async fn event_handler(&mut self) {
        graphics::lock!(self.graphics_manager, {
            {
                while let Some(event) = self.window.pop_event() {
                    // If we are typing the user name or the password
                    if event.code == EventKind::ValueChanged
                        && (event.target == self.user_name_text_area
                            || event.target == self.password_text_area)
                    {
                        self.clear_error();
                    }
                    // If the "Login" button is clicked
                    else if event.code == EventKind::Clicked && event.target == self.button {
                        let result = self.authenticate().await;

                        if let Err(error) = result {
                            self.print_error(error);
                        }
                    }
                }
            }
        })
    }

    pub fn get_logged_user(&self) -> Option<UserIdentifier> {
        self.user
    }
}
