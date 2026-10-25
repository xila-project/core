use crate::{Error, Result};
use getargs::Options;
use getargs_derive::Error as ArgumentError;
use xila::{
    file_system::{AccessFlags, CreateFlags, Flags, Path},
    virtual_file_system::File,
};

use super::{CommandContext, UserCommand, directory::resolve_path};

const HELP: &str =
    "Usage: touch FILE...\nCreate missing files and update access and modification times.";

pub struct TouchCommand;

impl UserCommand for TouchCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut Options<&'a str, I>,
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

        let mut supplied_path = false;
        while let Some(argument) = options.next_positional() {
            supplied_path = true;
            let path = resolve_path(context, argument)?;
            let virtual_file_system = context.executable_context().virtual_file_system;
            let file = File::open(
                virtual_file_system,
                context.task_id(),
                &path,
                Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
            )
            .await
            .map_err(Error::FailedToOpenFile)?;
            file.close(virtual_file_system)
                .await
                .map_err(Error::FailedToOpenFile)?;
        }

        if !supplied_path {
            return Err(Error::InvalidNumberOfArguments);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::check_no_more_options;

    #[test]
    fn accepts_multiple_positional_paths_and_rejects_missing_paths_or_options() {
        let mut options = Options::new(["first", "second"].into_iter());
        check_no_more_options(&mut options).unwrap();
        assert_eq!(options.next_positional(), Some("first"));
        assert_eq!(options.next_positional(), Some("second"));
        assert_eq!(options.next_positional(), None);

        let mut options = Options::new(["-R", "file"].into_iter());
        assert!(check_no_more_options(&mut options).is_err());
        let mut options = Options::new(core::iter::empty::<&str>());
        assert_eq!(options.next_positional(), None);
    }
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "macos", target_os = "windows")
))]
mod filesystem_tests {
    use super::*;
    use core::fmt;
    use xila::{
        executable::{ExecutableContext, Standard},
        file_system::{PathOwned, Statistics},
        task::TaskIdentifier,
        virtual_file_system::Directory,
    };

    struct Context {
        standard: Standard<'static>,
        task: TaskIdentifier,
    }

    impl CommandContext for Context {
        fn executable_context(&self) -> &'static ExecutableContext {
            self.standard.context
        }
        fn task_id(&self) -> TaskIdentifier {
            self.task
        }
        fn current_directory(&self) -> &Path {
            Path::from_str("/touch_test")
        }
        fn set_current_directory(&mut self, _: PathOwned) {
            panic!("unexpected directory change")
        }
        fn stop(&mut self) {
            panic!("unexpected exit")
        }
        async fn write_out_fmt(&mut self, _: fmt::Arguments<'_>) -> Result<()> {
            Ok(())
        }
        async fn write_out(&mut self, _: &[u8]) -> Result<()> {
            Ok(())
        }
        async fn write_out_line(&mut self, _: &[u8]) -> Result<()> {
            Ok(())
        }
        fn standard(&mut self) -> &mut Standard<'static> {
            &mut self.standard
        }
    }

    async fn touch<'a>(context: &mut Context, paths: &'a [&'a str]) -> Result<(), Error<'a>> {
        TouchCommand
            .execute(context, &mut Options::new(paths.iter().copied()), &[])
            .await
    }

    async fn statistics(context: &Context, path: &str) -> Statistics {
        context
            .executable_context()
            .virtual_file_system
            .get_statistics(&path)
            .await
            .unwrap()
    }

    #[xila::task::test(task_path = xila::task)]
    async fn creates_missing_files_touches_existing_files_and_resolves_relative_paths() {
        let standard = testing::initialize(false, false).await;
        let task = standard
            .context
            .task_manager
            .get_current_task_identifier()
            .await;
        let vfs = standard.context.virtual_file_system;
        Directory::create(vfs, task, "/touch_test").await.unwrap();
        let mut context = Context { standard, task };

        touch(&mut context, &["new", "also-new"]).await.unwrap();
        let created = statistics(&context, "/touch_test/new").await;
        assert_eq!(created.size, 0);
        assert_eq!(created.kind, xila::file_system::Kind::File);
        assert_eq!(statistics(&context, "/touch_test/also-new").await.size, 0);

        let mut existing = File::open(
            vfs,
            task,
            "/touch_test/new",
            Flags::new(AccessFlags::Write, None, None),
        )
        .await
        .unwrap();
        existing.write(b"preserve contents").await.unwrap();
        existing.close(vfs).await.unwrap();
        let before = statistics(&context, "/touch_test/new").await;

        touch(&mut context, &["new"]).await.unwrap();
        let after = statistics(&context, "/touch_test/new").await;
        assert_eq!(after.size, before.size);
        assert!(after.modification >= before.modification);
    }

    #[xila::task::test(task_path = xila::task)]
    async fn rejects_missing_arguments_invalid_paths_and_directories() {
        let standard = testing::initialize(false, false).await;
        let task = standard
            .context
            .task_manager
            .get_current_task_identifier()
            .await;
        let vfs = standard.context.virtual_file_system;
        Directory::create(vfs, task, "/touch_test").await.unwrap();
        Directory::create(vfs, task, "/touch_test/directory")
            .await
            .unwrap();
        let mut context = Context { standard, task };

        assert!(matches!(
            touch(&mut context, &[]).await,
            Err(Error::InvalidNumberOfArguments)
        ));
        assert!(matches!(
            touch(&mut context, &["bad\0path"]).await,
            Err(Error::InvalidPath)
        ));
        assert!(matches!(
            touch(&mut context, &["directory"]).await,
            Err(Error::FailedToOpenFile(_))
        ));
    }
}
