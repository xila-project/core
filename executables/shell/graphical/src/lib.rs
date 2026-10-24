#![no_std]

mod desk;
mod error;
mod home;
mod icon;
mod layout;
mod login;
mod shortcut;

extern crate alloc;

xila::internationalization::track_locales!();

use crate::{desk::Desk, error::Error};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::fmt::Write;
use core::num::NonZeroUsize;
use core::time::Duration;
use getargs_derive::GetArgs;
use home::Home;
use layout::Layout;
use login::Login;
use xila::executable::{self, ExecutableTrait, Standard};
use xila::graphics;
use xila::task;

#[derive(GetArgs)]
#[command(name = "graphical_shell", about = "Run the graphical shell.", version)]
struct GraphicalShellArguments {
    /// Show the on-screen keyboard.
    #[arg(short = 'k', long = "show-keyboard")]
    show_keyboard: bool,
}

pub async fn main(
    mut standard: Standard<'static>,
    arguments: Vec<String>,
) -> Result<(), NonZeroUsize> {
    let GraphicalShellArguments { show_keyboard } =
        match GraphicalShellArguments::from_args(arguments.iter().map(String::as_str)) {
            Ok(arguments) => arguments,
            Err(error) if error.is_help() || error.is_version() => {
                writeln!(standard.out(), "{error}").ok();
                return Ok(());
            }
            Err(error) => {
                writeln!(standard.error(), "{error}").ok();
                return Err(NonZeroUsize::new(1).unwrap());
            }
        };

    Shell::new(standard, show_keyboard).await.main().await
}

pub struct Shell {
    context: &'static xila::executable::ExecutableContext,
    _standard: Standard<'static>,
    running: bool,
    layout: Layout,
    desk: Option<Box<Desk>>,
    _home: Option<Box<Home>>,
    login: Option<Box<Login>>,
}

pub struct ShellExecutable;

impl ExecutableTrait for ShellExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(async move { main(standard, arguments).await })
    }
}

impl Shell {
    pub async fn new(standard: Standard<'static>, show_keyboard: bool) -> Self {
        let context = standard.context;
        let layout = Layout::new(context, show_keyboard).await.unwrap();

        let login = Box::new(Login::new(context).await.unwrap());

        Self {
            context,
            _standard: standard,
            layout,
            desk: None,
            running: true,
            _home: None,
            login: Some(login),
        }
    }

    pub async fn main(&mut self) -> Result<(), NonZeroUsize> {
        while self.running && !xila::lifecycle::SYSTEM.is_shutdown_requested() {
            self.layout.run().await;

            if let Some(login) = &mut self.login {
                login.event_handler().await;

                if let Some(user) = login.get_logged_user() {
                    let user_name = self
                        .context
                        .users_manager
                        .get_user_name(user)
                        .await
                        .unwrap();

                    let task_manager = &self.context.task_manager;
                    let task = task_manager.get_current_task_identifier().await;

                    graphics::appearance::restore_appearance(
                        graphics::ffi_manager(),
                        self.context.virtual_file_system,
                        task,
                    )
                    .await;

                    task_manager
                        .set_environment_variable(task, "User", user_name.as_str())
                        .await
                        .map_err(Error::FailedToSetEnvironmentVariable)?;

                    self.desk = Some(Box::new(
                        Desk::new(self.context, self.layout.get_windows_parent()).await?,
                    ));

                    if let Some(desk) = &mut self.desk {
                        self._home = Some(Box::new(
                            Home::new(graphics::ffi_manager(), desk.get_window_object()).await?,
                        ));
                    }

                    self.login = None;
                }
            }

            if let Some(desk) = &mut self.desk
                && !desk.is_hidden()
            {
                desk.handle_events().await;
            }

            task::sleep(Duration::from_millis(50)).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use getargs::Options;

    use super::GraphicalShellArguments;

    #[test]
    fn parses_show_keyboard_flag_without_panic() {
        let args = ["--show-keyboard"];
        let mut options = Options::new(args.into_iter());

        let parsed = GraphicalShellArguments::parse(&mut options);

        assert!(parsed.is_ok());
        assert!(parsed.unwrap().show_keyboard);
    }

    #[test]
    fn keyboard_is_hidden_by_default() {
        let parsed = GraphicalShellArguments::from_args([].into_iter()).unwrap();
        assert!(!parsed.show_keyboard);
    }

    #[test]
    fn help_and_version_are_information_requests() {
        for argument in ["--help", "-h", "--version", "-V"] {
            let error = GraphicalShellArguments::from_args([argument].into_iter())
                .err()
                .unwrap();
            assert!(error.is_help() || error.is_version());
        }
        assert!(GraphicalShellArguments::HELP.contains("Usage: graphical_shell"));
        assert!(GraphicalShellArguments::HELP.contains("--show-keyboard"));
    }
}
