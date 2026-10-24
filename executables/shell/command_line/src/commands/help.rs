use getargs_derive::GetArgs;
use xila::internationalization::translate;
use xila::{
    file_system::{Kind, Path},
    virtual_file_system::Directory,
};

use super::{BUILTINS, CommandContext, UserCommand};
use crate::{Error, Result};

pub struct HelpCommand;

#[derive(GetArgs)]
#[command(name = "help", about = "List commands or show help for a command.")]
pub(super) struct HelpArguments<'a> {
    #[arg(default_value = "")]
    command: &'a str,
}

impl UserCommand for HelpCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut getargs::Options<&'a str, I>,
        paths: &[&Path],
    ) -> Result<(), Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext,
    {
        let HelpArguments { command } = HelpArguments::parse(options)?;
        if let Some((_, _, help)) = BUILTINS.iter().find(|entry| entry.0 == command) {
            return context.write_out_fmt(format_args!("{help}\n")).await;
        }

        if !command.is_empty() {
            if visit_executables(context, paths, Some(command)).await? {
                return context
                    .write_out_fmt(format_args!(
                        "{}: {}\n",
                        command,
                        translate!("help.external_executable_description")
                    ))
                    .await;
            }
            return Err(Error::CommandNotFound);
        }

        context
            .write_out_fmt(format_args!(
                "{}\n",
                translate!("help.built_in_commands_title")
            ))
            .await?;
        for (name, _, _) in BUILTINS {
            context.write_out_fmt(format_args!("{name}\n")).await?;
        }
        context
            .write_out_fmt(format_args!(
                "{}\n",
                translate!("help.available_executables_title")
            ))
            .await?;
        visit_executables(context, paths, None).await?;
        Ok(())
    }
}

/// Stream search-path entries without starting or opening executable devices.
/// Missing/inaccessible directories are skipped as in command resolution; read
/// and metadata failures are reported. Names in multiple paths can appear more
/// than once, keeping memory bounded to one directory entry and joined path.
/// With a query, stop at the first matching executable instead of printing.
async fn visit_executables<C: CommandContext>(
    context: &mut C,
    paths: &[&Path],
    query: Option<&str>,
) -> Result<bool> {
    for path in paths {
        let executable_context = context.executable_context();
        let vfs = &executable_context.virtual_file_system;
        let task = context.task_id();
        let Ok(mut directory) = Directory::open(vfs, task, path).await else {
            continue;
        };
        let user = executable_context
            .task_manager
            .get_user(task)
            .await
            .map_err(Error::FailedToGetTaskUser)?;
        while let Some(entry) = directory
            .read()
            .await
            .map_err(Error::FailedToReadDirectoryEntry)?
        {
            if query.is_some_and(|query| query != entry.name)
                || BUILTINS.iter().any(|builtin| builtin.0 == entry.name)
                || !matches!(
                    entry.kind,
                    Kind::File | Kind::CharacterDevice | Kind::SymbolicLink
                )
            {
                continue;
            }
            let entry_path = entry.join_path(path).ok_or(Error::FailedToJoinPath)?;
            let statistics = vfs
                .get_statistics(&entry_path)
                .await
                .map_err(Error::FailedToGetMetadata)?;
            if !xila::executable::is_execute_allowed(
                executable_context.users_manager,
                &statistics,
                user,
            )
            .await
            {
                continue;
            }
            if query.is_some() {
                return Ok(true);
            }
            context
                .write_out_fmt(format_args!("{}\n", entry.name))
                .await?;
        }
    }
    Ok(false)
}
