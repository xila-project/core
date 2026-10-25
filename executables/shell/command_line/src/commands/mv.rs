use crate::{Error, Result};
use getargs_derive::GetArgs;
use xila::file_system::Path;

use super::{CommandContext, UserCommand, directory::resolve_path};

pub struct MoveCommand;

#[derive(GetArgs)]
#[command(
    name = "mv",
    about = "Rename a file or directory to the destination path. Both paths must be on the same mounted filesystem."
)]
pub(super) struct MoveArguments<'a> {
    source: &'a str,
    destination: &'a str,
}

impl UserCommand for MoveCommand {
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
        let MoveArguments {
            source,
            destination,
        } = MoveArguments::parse(options)?;
        let source = resolve_path(context, source)?;
        let destination = resolve_path(context, destination)?;

        context
            .executable_context()
            .virtual_file_system
            .rename(context.task_id(), &source, &destination)
            .await
            .map_err(Error::FailedToRename)
    }
}

#[cfg(test)]
mod tests {
    use super::MoveArguments;

    #[test]
    fn move_requires_exactly_two_paths_and_rejects_options() {
        for args in [
            alloc::vec![],
            alloc::vec!["source"],
            alloc::vec!["source", "destination", "extra"],
            alloc::vec!["-f", "source"],
        ] {
            assert!(MoveArguments::parse(&mut getargs::Options::new(args.into_iter())).is_err());
        }
    }
}
