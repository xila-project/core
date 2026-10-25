//! Privileged power control through the VFS instead of host syscalls.
use getargs_derive::GetArgs;
use xila::{
    file_system::{AccessFlags, Path},
    lifecycle::REQUEST_SHUTDOWN,
    virtual_file_system::File,
};

use super::{CommandContext, UserCommand};
use crate::{Error, Result};

pub struct ShutdownCommand;

#[derive(GetArgs)]
#[command(
    name = "shutdown",
    about = "Stop Xila gracefully, without powering off the host computer."
)]
pub(super) struct ShutdownArguments {}

impl UserCommand for ShutdownCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut getargs::Options<&'a str, I>,
        _paths: &[&Path],
    ) -> Result<(), Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext,
    {
        let _ = ShutdownArguments::parse(options)?;
        let exec = context.executable_context();
        let mut device = File::open(
            exec.virtual_file_system,
            context.task_id(),
            "/devices/power/control",
            AccessFlags::Write.into(),
        )
        .await
        .map_err(Error::FailedToOpenFile)?;
        device
            .control(REQUEST_SHUTDOWN, &())
            .await
            .map_err(Error::FailedToOpenFile)?;
        device
            .close(exec.virtual_file_system)
            .await
            .map_err(Error::FailedToOpenFile)?;
        context.stop();
        Ok(())
    }
}
