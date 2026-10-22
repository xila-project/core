use crate::{Error, Result};
use alloc::{borrow::ToOwned, vec::Vec};
use getargs_derive::GetArgs;
use xila::{
    file_system::{Kind, Path, PathOwned},
    task::TaskIdentifier,
    virtual_file_system::{Directory, VirtualFileSystem},
};

use super::{CommandContext, UserCommand};

const MAX_DEPTH: usize = 32;
const MAX_PATH_BYTES: usize = 1024;

pub struct FindCommand;

#[derive(GetArgs)]
#[command(
    name = "find",
    about = "Find paths by name or entry kind. Traversal depth is limited to 32 and paths to 1024 bytes."
)]
pub(super) struct FindArguments<'a> {
    #[arg(default_value = "")]
    path: &'a str,
    #[arg(short = 'n', long = "name")]
    name: Option<&'a str>,
    #[arg(short = 't', long = "type")]
    kind: Option<&'a str>,
    #[arg(short = 'd', long = "max-depth", default_value_t = 32)]
    max_depth: usize,
}

impl UserCommand for FindCommand {
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
        let FindArguments {
            path,
            name,
            kind,
            max_depth,
        } = FindArguments::parse(options)?;
        if max_depth > MAX_DEPTH {
            return Err(Error::InvalidArgument);
        }
        let kind = kind.map(parse_kind).transpose()?;
        let path = resolve_path(context.current_directory(), path)?;
        let vfs = context.executable_context().virtual_file_system;
        let task = context.task_id();
        walk(vfs, task, path, name, kind, max_depth, async |path| {
            context.write_out_fmt(format_args!("{path}\n")).await
        })
        .await
    }
}

fn parse_kind(kind: &str) -> Result<Kind> {
    match kind {
        "f" => Ok(Kind::File),
        "d" => Ok(Kind::Directory),
        "c" => Ok(Kind::CharacterDevice),
        "b" => Ok(Kind::BlockDevice),
        "p" => Ok(Kind::Pipe),
        "s" => Ok(Kind::Socket),
        "l" => Ok(Kind::SymbolicLink),
        _ => Err(Error::InvalidArgument),
    }
}

fn resolve_path(base: &Path, path: &str) -> Result<PathOwned> {
    if !Path::from_str(path).is_valid() || path.len() > MAX_PATH_BYTES {
        return Err(Error::InvalidPath);
    }
    let path = Path::from_str(path);
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        if base.as_str().len() + path.as_str().len() > MAX_PATH_BYTES {
            return Err(Error::InvalidPath);
        }
        base.to_owned().join(path).ok_or(Error::FailedToJoinPath)?
    }
    .canonicalize();
    if path.as_str().len() > MAX_PATH_BYTES {
        return Err(Error::InvalidPath);
    }
    Ok(path)
}

fn matches(
    name: &str,
    entry_kind: Kind,
    wanted_name: Option<&str>,
    wanted_kind: Option<Kind>,
) -> bool {
    wanted_name.is_none_or(|wanted| name == wanted)
        && wanted_kind.is_none_or(|wanted| entry_kind == wanted)
}

/// Walks in filesystem order, keeping one directory handle per active level and
/// a single bounded path buffer. Root and nested directory failures stop the
/// command with the underlying VFS error.
async fn walk(
    vfs: &VirtualFileSystem,
    task: TaskIdentifier,
    mut path: PathOwned,
    wanted_name: Option<&str>,
    wanted_kind: Option<Kind>,
    max_depth: usize,
    mut write_match: impl AsyncFnMut(&str) -> Result<()>,
) -> Result<()> {
    if max_depth > MAX_DEPTH {
        return Err(Error::InvalidArgument);
    }
    if path.as_str().len() > MAX_PATH_BYTES {
        return Err(Error::InvalidPath);
    }

    let root_statistics = vfs
        .get_statistics(&path)
        .await
        .map_err(Error::FailedToGetMetadata)?;
    let root_name = path.get_file_name();

    if root_statistics.kind != Kind::Directory {
        if matches(root_name, root_statistics.kind, wanted_name, wanted_kind) {
            write_match(path.as_str()).await?;
        }
        return Ok(());
    }

    let root = Directory::open(vfs, task, &path)
        .await
        .map_err(Error::FailedToOpenDirectory)?;
    if matches(root_name, Kind::Directory, wanted_name, wanted_kind) {
        write_match(path.as_str()).await?;
    }
    if max_depth == 0 {
        return root.close(vfs).await.map_err(Error::FailedToOpenDirectory);
    }

    let mut stack = Vec::with_capacity(max_depth + 1);
    stack.push((root, path.as_str().len()));
    while let Some((directory, _)) = stack.last_mut() {
        let entry = directory
            .read()
            .await
            .map_err(Error::FailedToReadDirectoryEntry)?;
        let Some(entry) = entry else {
            let (directory, parent_length) = stack.pop().unwrap();
            directory
                .close(vfs)
                .await
                .map_err(Error::FailedToOpenDirectory)?;
            path.truncate(parent_length);
            continue;
        };
        if entry.name == "." || entry.name == ".." {
            continue;
        }
        let depth = stack.len();
        let parent_length = path.as_str().len();
        if parent_length + entry.name.len() + 1 > MAX_PATH_BYTES {
            return Err(Error::InvalidPath);
        }
        path = path.append(&entry.name).ok_or(Error::FailedToJoinPath)?;
        if matches(&entry.name, entry.kind, wanted_name, wanted_kind) {
            write_match(path.as_str()).await?;
        }
        if entry.kind == Kind::Directory && depth < max_depth {
            let child = Directory::open(vfs, task, &path)
                .await
                .map_err(Error::FailedToOpenDirectory)?;
            stack.push((child, parent_length));
        } else {
            path.truncate(parent_length);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use core::fmt::Write;

    #[test]
    fn parses_filters_and_depth() {
        let args = FindArguments::from_args([].into_iter()).unwrap();
        assert_eq!(args.path, "");
        assert_eq!(args.max_depth, 32);
        assert_eq!(args.name, None);
        assert_eq!(args.kind, None);
        let args =
            FindArguments::from_args(["-n", "note.txt", "-t", "f", "-d", "2", "/docs"].into_iter())
                .unwrap();
        assert_eq!(args.path, "/docs");
        assert_eq!(args.name, Some("note.txt"));
        assert_eq!(args.kind, Some("f"));
        assert_eq!(args.max_depth, 2);
    }

    #[test]
    fn accepts_known_types_and_rejects_unknown_types() {
        assert_eq!(parse_kind("f").unwrap(), Kind::File);
        assert_eq!(parse_kind("d").unwrap(), Kind::Directory);
        assert!(parse_kind("x").is_err());
    }

    #[test]
    fn resolves_relative_and_absolute_paths() {
        let base = Path::from_str("/work");
        assert_eq!(resolve_path(base, "").unwrap().as_str(), "/work");
        assert_eq!(
            resolve_path(base, "nested").unwrap().as_str(),
            "/work/nested"
        );
        assert_eq!(resolve_path(base, "/other").unwrap().as_str(), "/other");
        assert!(resolve_path(base, "bad?name").is_err());
    }

    #[xila::task::test(task_path = xila::task)]
    async fn finds_nested_entries_with_filters_and_depth() {
        let standard = testing::initialize(false, false).await;
        let vfs = standard.context.virtual_file_system;
        let task = standard
            .context
            .task_manager
            .get_current_task_identifier()
            .await;
        for path in ["/find_test", "/find_test/sub", "/find_test/sub/deep"] {
            Directory::create(vfs, task, Path::from_str(path))
                .await
                .unwrap();
        }
        for path in [
            "/find_test/note.txt",
            "/find_test/sub/note.txt",
            "/find_test/sub/deep/other.txt",
        ] {
            let file = xila::virtual_file_system::File::open(
                vfs,
                task,
                Path::from_str(path),
                xila::file_system::Flags::new(
                    xila::file_system::AccessFlags::Write,
                    Some(xila::file_system::CreateFlags::Create),
                    None,
                ),
            )
            .await
            .unwrap();
            file.close(vfs).await.unwrap();
        }

        let mut output = String::new();
        walk(
            vfs,
            task,
            Path::from_str("/find_test").to_owned(),
            Some("note.txt"),
            Some(Kind::File),
            32,
            async |path| {
                writeln!(output, "{path}").unwrap();
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(output.contains("/find_test/note.txt\n"));
        assert!(output.contains("/find_test/sub/note.txt\n"));
        assert!(!output.contains("other.txt"));

        let mut depth_output = String::new();
        walk(
            vfs,
            task,
            Path::from_str("/find_test").to_owned(),
            None,
            Some(Kind::File),
            1,
            async |path| {
                writeln!(depth_output, "{path}").unwrap();
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(depth_output.contains("/find_test/note.txt\n"));
        assert!(!depth_output.contains("/find_test/sub/note.txt"));
    }

    #[xila::task::test(task_path = xila::task)]
    async fn reports_missing_and_inaccessible_roots() {
        let standard = testing::initialize(false, false).await;
        let vfs = standard.context.virtual_file_system;
        let task = standard
            .context
            .task_manager
            .get_current_task_identifier()
            .await;
        assert!(matches!(
            walk(
                vfs,
                task,
                Path::from_str("/find_missing").to_owned(),
                None,
                None,
                32,
                async |_| Ok(())
            )
            .await,
            Err(Error::FailedToGetMetadata(_))
        ));

        Directory::create(vfs, task, Path::from_str("/find_denied"))
            .await
            .unwrap();
        vfs.set_permissions(
            task,
            Path::from_str("/find_denied"),
            xila::file_system::Permissions::NONE,
        )
        .await
        .unwrap();
        standard
            .context
            .task_manager
            .set_user(task, xila::users::UserIdentifier::MINIMUM)
            .await
            .unwrap();
        let result = walk(
            vfs,
            task,
            Path::from_str("/find_denied").to_owned(),
            None,
            None,
            32,
            async |_| Ok(()),
        )
        .await;
        standard
            .context
            .task_manager
            .set_user(task, xila::users::UserIdentifier::ROOT)
            .await
            .unwrap();
        assert!(matches!(result, Err(Error::FailedToOpenDirectory(_))));
    }
}
