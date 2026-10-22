#![no_std]
extern crate alloc;
#[cfg(test)]
extern crate std;
xila::internationalization::track_locales!();

use alloc::{
    borrow::ToOwned,
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::num::NonZeroUsize;
use xila::{
    executable::{self, ExecutableTrait, Standard},
    file_system::{Path, PathOwned},
    task::TaskIdentifier,
};
mod commands;
mod error;
mod execution;
mod parser;
mod resolver;
mod tokenizer;
use commands::CommandContext;
pub use error::*;

pub struct Shell {
    context: &'static executable::ExecutableContext,
    task: TaskIdentifier,
    standard: Standard<'static>,
    current_directory: PathOwned,
    running: bool,
    user: String,
    host: String,
    last_status: usize,
    background_jobs: Vec<xila::task::JoinHandle<usize>>,
}
pub struct ShellExecutable;

/// Reused per built-in, bounded even if a Display implementation emits large text.
#[derive(Default)]
struct OutputBuffer(String);
impl core::fmt::Write for OutputBuffer {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        if self.0.len() + value.len() > execution::MAX_EXPANDED_BYTES {
            return Err(core::fmt::Error);
        }
        self.0.push_str(value);
        Ok(())
    }
}
impl OutputBuffer {
    async fn write(
        &mut self,
        file: &mut xila::virtual_file_system::File<'_>,
        arguments: core::fmt::Arguments<'_>,
    ) -> Result<()> {
        self.0.clear();
        core::fmt::write(self, arguments)?;
        execution::write_all(file, self.0.as_bytes()).await
    }
}
struct ShellCommandContext<'a> {
    shell: &'a mut Shell,
    output: OutputBuffer,
}
impl<'a> ShellCommandContext<'a> {
    fn new(shell: &'a mut Shell) -> Self {
        Self {
            shell,
            output: OutputBuffer::default(),
        }
    }
}
impl CommandContext for ShellCommandContext<'_> {
    fn executable_context(&self) -> &'static executable::ExecutableContext {
        self.shell.context
    }
    fn task_id(&self) -> TaskIdentifier {
        self.shell.task
    }
    fn current_directory(&self) -> &Path {
        &self.shell.current_directory
    }
    fn set_current_directory(&mut self, directory: PathOwned) {
        self.shell.current_directory = directory;
    }
    fn stop(&mut self) {
        self.shell.running = false;
    }
    async fn write_out_fmt(&mut self, arguments: core::fmt::Arguments<'_>) -> Result<()> {
        self.output
            .write(self.shell.standard.out(), arguments)
            .await
    }
    async fn write_out(&mut self, buffer: &[u8]) -> Result<()> {
        execution::write_all(self.shell.standard.out(), buffer).await
    }
    async fn write_out_line(&mut self, buffer: &[u8]) -> Result<()> {
        self.write_out(buffer).await?;
        self.write_out(b"\n").await
    }
    fn standard(&mut self) -> &mut Standard<'static> {
        &mut self.shell.standard
    }
}
impl ExecutableTrait for ShellExecutable {
    fn main(standard: Standard<'static>, arguments: Vec<String>) -> executable::MainFuture {
        Box::pin(async move { main(standard, arguments).await })
    }
}
pub async fn main(
    standard: Standard<'static>,
    arguments: Vec<String>,
) -> core::result::Result<(), NonZeroUsize> {
    Shell::new(standard).await.main(arguments).await
}
impl Shell {
    pub async fn new(standard: Standard<'static>) -> Self {
        let context = standard.context;
        Self {
            context,
            standard,
            task: context.task_manager.get_current_task_identifier().await,
            current_directory: Path::ROOT.to_owned(),
            running: true,
            user: String::new(),
            host: String::new(),
            last_status: 0,
            background_jobs: Vec::new(),
        }
    }
    async fn diagnostic(&mut self, error: impl core::fmt::Display) {
        let _ = OutputBuffer::default()
            .write(self.standard.error(), format_args!("{error}\n"))
            .await;
    }
    async fn main_interactive(&mut self) -> Result<()> {
        let mut input = Vec::with_capacity(64);
        while self.running {
            let _ = OutputBuffer::default()
                .write(
                    self.standard.out(),
                    format_args!("{}@{}:{}$ ", self.user, self.host, self.current_directory),
                )
                .await;
            let _ = self.standard.out().flush().await;
            input.clear();
            let mut oversized = false;
            let mut end_of_input = false;
            loop {
                let mut byte = [0u8; 1];
                let count = self
                    .standard
                    .input()
                    .read(&mut byte)
                    .await
                    .map_err(Error::FailedToReadFile)?;
                if count == 0 {
                    end_of_input = true;
                    break;
                }
                if byte[0] == b'\n' {
                    break;
                }
                if input.len() == tokenizer::MAX_INPUT_BYTES {
                    oversized = true;
                }
                if !oversized {
                    input.push(byte[0]);
                }
            }
            if oversized {
                self.last_status = 2;
                self.diagnostic(xila::internationalization::translate!(
                    "Command line is too long"
                ))
                .await;
                continue;
            }
            match core::str::from_utf8(&input) {
                Ok(source) => {
                    if let Err(error) = self.execute_line(source).await {
                        self.last_status = 2;
                        self.diagnostic(error).await;
                    }
                }
                Err(_) => {
                    self.last_status = 2;
                    self.diagnostic(xila::internationalization::translate!(
                        "Invalid UTF-8 input"
                    ))
                    .await;
                }
            }
            if end_of_input {
                self.running = false;
            }
        }
        Ok(())
    }
    pub async fn main(&mut self, arguments: Vec<String>) -> core::result::Result<(), NonZeroUsize> {
        let manager = self.context.task_manager;
        self.user = match manager.get_environment_variable(self.task, "User").await {
            Ok(user) => user.get_value().to_string(),
            Err(_) if !arguments.is_empty() => self
                .context
                .users_manager
                .get_user_name(
                    manager
                        .get_user(self.task)
                        .await
                        .map_err(Error::FailedToSetTaskUser)?,
                )
                .await
                .map_err(|_| Error::FailedToGetTaskIdentifier)?,
            Err(_) => loop {
                match self.authenticate().await {
                    Ok(Some(user)) => break user,
                    Ok(None) => return Ok(()),
                    Err(error) => self.diagnostic(error).await,
                }
            },
        };
        self.host = manager
            .get_environment_variable(self.task, "Host")
            .await
            .map_err(|_| Error::FailedToGetPath)?
            .get_value()
            .to_string();
        if arguments.is_empty() {
            self.main_interactive().await?;
        } else if arguments[0] == "-c" {
            if arguments.len() != 2 {
                self.diagnostic(xila::internationalization::translate!(
                    "Usage: shell -c COMMAND"
                ))
                .await;
                self.last_status = 2;
            } else if let Err(error) = self.execute_line(&arguments[1]).await {
                self.last_status = 2;
                self.diagnostic(error).await;
            }
        } else {
            // Already separated arguments remain literal; do not parse caller-provided data as shell syntax.
            self.last_status = self.run_arguments(&arguments).await;
        }
        // Keep task/stream ownership explicit; an interactive prompt is free while jobs run.
        for job in self.background_jobs.drain(..) {
            job.join().await;
        }
        if self.standard.out().flush().await.is_err()
            || self.standard.error().flush().await.is_err()
        {
            self.last_status = 1;
        }
        match NonZeroUsize::new(self.last_status) {
            Some(status) => Err(status),
            None => Ok(()),
        }
    }
}
