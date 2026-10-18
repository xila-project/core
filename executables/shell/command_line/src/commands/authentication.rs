use crate::{Result, Shell, error::Error};
use alloc::string::String;
use core::fmt::Write;
use xila::{authentication, internationalization::translate};

impl Shell {
    pub async fn authenticate(&mut self) -> Result<String> {
        write!(self.standard.out(), translate!("User name: "))?;
        let _ = self.standard.out().flush().await;

        let mut user_name = String::with_capacity(32);
        self.standard.read_line(&mut user_name).await.unwrap();

        write!(self.standard.out(), translate!("Password: "))?;
        let _ = self.standard.out().flush().await;

        let mut password = String::with_capacity(32);
        self.standard.read_line(&mut password).await.unwrap();

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

        Ok(user_name)
    }
}
