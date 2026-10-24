use crate::{Result, Shell, error::Error};
use alloc::string::String;
use core::fmt::Write;
use xila::{authentication, internationalization::translate};

impl Shell {
    // Unlike Standard::read_line, a login prompt must stop at EOF rather than
    // repeatedly retrying an already closed stdin (e.g. docker run without -i).
    async fn read_login_line(&mut self, buffer: &mut String) -> Result<bool> {
        buffer.clear();
        let mut byte = [0u8; 1];
        loop {
            let count = self
                .standard
                .input()
                .read(&mut byte)
                .await
                .map_err(Error::FailedToReadFile)?;
            if count == 0 {
                return Ok(false);
            }
            if byte[0] == b'\n' {
                return Ok(true);
            }
            if buffer.len() >= 256 {
                return Err(Error::LineTooLong);
            }
            buffer.push(char::from(byte[0]));
        }
    }

    pub async fn authenticate(&mut self) -> Result<Option<String>> {
        write!(
            self.standard.out(),
            translate!("authentication.prompts.username")
        )?;
        let _ = self.standard.out().flush().await;

        let mut user_name = String::with_capacity(32);
        if !self.read_login_line(&mut user_name).await? {
            return Ok(None);
        }

        write!(
            self.standard.out(),
            translate!("authentication.prompts.password")
        )?;
        let _ = self.standard.out().flush().await;

        let mut password = String::with_capacity(32);
        if !self.read_login_line(&mut password).await? {
            return Ok(None);
        }

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
            authentication::authenticate_user(&authentication_context, &user_name, &password)
                .await
                .map_err(Error::AuthenticationFailed)?;

        let home = self
            .context
            .virtual_file_system
            .ensure_user_home(task, user_identifier)
            .await
            .map_err(Error::FailedToCreateDirectory)?;
        task_manager
            .set_environment_variable(task, "HOME", home.as_str())
            .await
            .map_err(Error::FailedToSetEnvironmentVariable)?;

        // - Set the user
        task_manager
            .set_user(task, user_identifier)
            .await
            .map_err(Error::FailedToSetTaskUser)?;

        task_manager
            .set_environment_variable(task, "User", &user_name)
            .await
            .map_err(Error::FailedToSetEnvironmentVariable)?;

        Ok(Some(user_name))
    }
}
