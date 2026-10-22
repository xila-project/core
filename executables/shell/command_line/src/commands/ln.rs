use crate::{Error, Result};
use alloc::vec;
use getargs_derive::{Error as ArgumentError, GetArgs};
use xila::file_system::Path;

use super::{CommandContext, UserCommand, directory::resolve_path};

pub struct LinkCommand;

const HELP: &str = "Usage: ln TARGET LINK\nCreate a symbolic link.";

#[derive(GetArgs)]
#[command(name = "ln", about = "Create a symbolic link.")]
struct LinkArguments<'a> {
    target: &'a str,
    link: &'a str,
}

impl UserCommand for LinkCommand {
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
        match options.next_opt() {
            Ok(None) => {}
            Ok(Some(getargs::Opt::Long("help"))) | Ok(Some(getargs::Opt::Short('h'))) => {
                return Err(Error::ArgumentParsing(ArgumentError::Help(HELP)));
            }
            _ => return Err(Error::InvalidOption),
        }
        let LinkArguments { target, link } = LinkArguments::parse(options)?;
        let link = resolve_path(context, link)?;
        context
            .executable_context()
            .virtual_file_system
            .create_symlink(context.task_id(), Path::from_str(target), &link)
            .await
            .map_err(Error::FailedToCreateSymbolicLink)
    }
}

pub struct ReadLinkCommand;

impl UserCommand for ReadLinkCommand {
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
        match options.next_opt() {
            Ok(None) => {}
            Ok(Some(getargs::Opt::Long("help"))) | Ok(Some(getargs::Opt::Short('h'))) => {
                return Err(Error::ArgumentParsing(ArgumentError::Help(
                    "Usage: readlink LINK\nPrint a symbolic link target.",
                )));
            }
            _ => return Err(Error::InvalidOption),
        };
        let Some(path) = options.next_positional() else {
            return Err(Error::InvalidNumberOfArguments);
        };
        if options.next_positional().is_some() {
            return Err(Error::InvalidNumberOfArguments);
        }
        let path = resolve_path(context, path)?;
        let mut target = vec![0; 4096];
        let length = context
            .executable_context()
            .virtual_file_system
            .read_symlink(context.task_id(), &path, &mut target)
            .await
            .map_err(Error::FailedToReadSymbolicLink)?;
        context.write_out_line(&target[..length]).await
    }
}

#[cfg(test)]
mod tests {
    use super::LinkArguments;

    #[test]
    fn link_requires_target_and_link_paths() {
        assert!(LinkArguments::parse(&mut getargs::Options::new(["target"].into_iter())).is_err());
        assert!(
            LinkArguments::parse(&mut getargs::Options::new(
                ["target", "link", "extra"].into_iter()
            ))
            .is_err()
        );
    }
}
