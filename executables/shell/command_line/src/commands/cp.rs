use crate::{Error, Result};
use getargs_derive::GetArgs;
use xila::{
    file_system::{AccessFlags, CreateFlags, Flags, Kind, Path},
    virtual_file_system::File,
};

use super::{CommandContext, UserCommand, directory::resolve_path};

pub struct CopyCommand;

#[derive(GetArgs)]
#[command(name = "cp", about = "Copy a file to a destination path or directory.")]
pub(super) struct CopyArguments<'a> {
    source: &'a str,
    destination: &'a str,
}
impl UserCommand for CopyCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut getargs::Options<&'a str, I>,
        _paths: &[&Path],
    ) -> Result<(), crate::Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext,
    {
        let CopyArguments {
            source,
            destination,
        } = CopyArguments::parse(options)?;
        let source = resolve_path(context, source)?;
        let mut destination = resolve_path(context, destination)?;
        let virtual_file_system = context.executable_context().virtual_file_system;
        let task = context.task_id();

        let source_statistics = virtual_file_system
            .get_statistics(&source)
            .await
            .map_err(Error::FailedToGetMetadata)?;
        if source_statistics.kind != Kind::File {
            return Err(Error::InvalidArgument);
        }

        if let Ok(statistics) = virtual_file_system.get_statistics(&destination).await {
            if statistics.kind == Kind::Directory {
                destination = destination
                    .join(Path::from_str(source.get_file_name()))
                    .ok_or(Error::FailedToJoinPath)?;
            }
        }

        if source == destination {
            return Err(Error::InvalidArgument);
        }

        let mut input = File::open(
            virtual_file_system,
            task,
            &source,
            Flags::new(AccessFlags::Read, None, None),
        )
        .await
        .map_err(Error::FailedToOpenFile)?;
        let mut output = File::open(
            virtual_file_system,
            task,
            &destination,
            Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
        )
        .await
        .map_err(Error::FailedToOpenFile)?;

        let mut buffer = [0u8; 512];
        loop {
            let count = input
                .read(&mut buffer)
                .await
                .map_err(Error::FailedToReadFile)?;
            if count == 0 {
                break;
            }
            write_all(&mut output, &buffer[..count]).await?;
        }

        input
            .close(virtual_file_system)
            .await
            .map_err(Error::FailedToReadFile)?;
        output
            .close(virtual_file_system)
            .await
            .map_err(Error::FailedToWriteFile)
    }
}

async fn write_all(file: &mut File<'_>, buffer: &[u8]) -> Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        let written = file
            .write(&buffer[offset..])
            .await
            .map_err(Error::FailedToWriteFile)?;
        if written == 0 {
            return Err(Error::FailedToWriteFile(
                xila::virtual_file_system::Error::FailedToWrite,
            ));
        }
        offset += written;
    }
    Ok(())
}
