use crate::{Error, Result};
use alloc::borrow::ToOwned;
use getargs::Arg;
use xila::file_system::{Path, PathOwned};

use super::{CommandContext, UserCommand};

pub struct RealpathCommand;

impl UserCommand for RealpathCommand {
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
        let mut had_operand = false;
        while let Some(argument) = options.next_arg().map_err(|_| Error::InvalidOption)? {
            match argument {
                Arg::Long("help") | Arg::Short('h') => {
                    context
                        .write_out_fmt(format_args!("{}\n", RealpathArguments::HELP))
                        .await?;
                    return Ok(());
                }
                Arg::Positional(path) => {
                    let path = resolve_path(context.current_directory(), path)?;
                    context
                        .executable_context()
                        .virtual_file_system
                        .get_statistics(&path)
                        .await
                        .map_err(Error::FailedToGetMetadata)?;
                    context
                        .write_out_fmt(format_args!("{}\n", path.as_str()))
                        .await?;
                    had_operand = true;
                }
                _ => return Err(Error::InvalidOption),
            }
        }

        if !had_operand {
            return Err(Error::MissingPositionalArgument("path"));
        }

        Ok(())
    }
}

pub(super) struct RealpathArguments;

impl RealpathArguments {
    pub const HELP: &'static str =
        "Usage: realpath PATH...\nPrint canonical paths for existing files and directories.";
}

fn resolve_path(base: &Path, path: &str) -> Result<PathOwned> {
    let path = Path::from_str(path);
    if !path.is_valid() {
        return Err(Error::InvalidPath);
    }

    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        base.to_owned().join(path).ok_or(Error::FailedToJoinPath)?
    };

    let is_absolute = path.is_absolute();
    let path = path.canonicalize();
    if is_absolute && !path.is_absolute() {
        return PathOwned::root()
            .join(Path::from_str(path.as_str()))
            .ok_or(Error::FailedToJoinPath);
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::resolve_path;
    use xila::file_system::Path;

    #[test]
    fn resolves_and_canonicalizes_relative_and_absolute_paths() {
        let base = Path::from_str("/home/user");

        assert_eq!(
            resolve_path(base, "../docs/./readme").unwrap().as_str(),
            "/home/docs/readme"
        );
        assert_eq!(resolve_path(base, "/tmp/../var").unwrap().as_str(), "/var");
        assert_eq!(resolve_path(base, "/../../tmp").unwrap().as_str(), "/tmp");
        assert_eq!(
            resolve_path(Path::from_str("/"), "../../tmp")
                .unwrap()
                .as_str(),
            "/tmp"
        );
    }

    #[test]
    fn rejects_invalid_paths() {
        assert!(resolve_path(Path::from_str("/home"), "bad?name").is_err());
    }
}
