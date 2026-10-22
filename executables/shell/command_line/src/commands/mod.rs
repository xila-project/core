mod authentication;
mod change_directory;
mod chmod;
mod clear;
mod concatenate;
mod cp;
mod directory;
mod dns;
mod echo;
mod environment_variables;
mod execute;
mod exit;
mod find;
mod grep;
mod head;
mod help;
mod ip;
mod list;
mod ln;
mod mv;
mod ping;
mod print_working_directory;
mod realpath;
mod sleep;
mod statistics;
mod tail;
mod touch;
mod tree;
mod web_request;
mod which;
mod word_count;

use crate::Result;
use alloc::borrow::ToOwned;
use core::fmt;
use xila::file_system::Path;
use xila::{
    executable::{ExecutableContext, Standard},
    file_system::PathOwned,
    task::TaskIdentifier,
};

use self::{
    change_directory::ChangeDirectoryCommand,
    chmod::ChmodCommand,
    clear::ClearCommand,
    concatenate::ConcatenateCommand,
    cp::CopyCommand,
    directory::{CreateDirectoryCommand, RemoveDirectoryCommand, RemoveFileCommand},
    dns::DnsResolveCommand,
    echo::EchoCommand,
    environment_variables::{
        PrintEnvironmentVariableCommand, RemoveEnvironmentVariableCommand,
        SetEnvironmentVariableCommand,
    },
    exit::ExitCommand,
    find::FindCommand,
    head::HeadCommand,
    ip::IpCommand,
    list::ListCommand,
    mv::MoveCommand,
    ping::PingCommand,
    print_working_directory::PrintWorkingDirectoryCommand,
    sleep::SleepCommand,
    statistics::StatisticsCommand,
    tail::TailCommand,
    touch::TouchCommand,
    tree::TreeCommand,
    web_request::WebRequestCommand,
    which::WhichCommand,
    word_count::WordCountCommand,
};

pub trait CommandContext {
    fn executable_context(&self) -> &'static ExecutableContext;
    fn task_id(&self) -> TaskIdentifier;
    fn current_directory(&self) -> &Path;
    fn set_current_directory(&mut self, directory: PathOwned);
    fn stop(&mut self);
    async fn write_out_fmt(&mut self, arguments: fmt::Arguments<'_>) -> Result<()>;
    async fn write_out(&mut self, buffer: &[u8]) -> Result<()>;
    async fn write_out_line(&mut self, buffer: &[u8]) -> Result<()>;
    fn standard(&mut self) -> &mut Standard<'static>;
    fn current_directory_owned(&self) -> PathOwned {
        self.current_directory().to_owned()
    }
}

pub trait UserCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut getargs::Options<&'a str, I>,
        paths: &[&Path],
    ) -> Result<(), crate::Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext;
}

#[derive(Clone, Copy)]
pub enum UserCommandKind {
    True,
    False,
    Exit,
    ChangeDirectory,
    Echo,
    List,
    Clear,
    Chmod,
    Concatenate,
    Grep,
    Statistics,
    CreateDirectory,
    SetEnvironmentVariable,
    RemoveEnvironmentVariable,
    RemoveFile,
    RemoveDirectory,
    WebRequest,
    DnsResolve,
    Ping,
    Ip,
    PrintWorkingDirectory,
    Realpath,
    PrintEnvironmentVariable,
    Which,
    WordCount,
    Head,
    Tail,
    Touch,
    Sleep,
    Help,
    Tree,
    Move,
    Copy,
    Link,
    ReadLink,
    Find,
}

/// Names and help text shared by command resolution and `help`.
/// Register new built-ins here so they are discoverable in the shell.
const BUILTINS: &[(&str, UserCommandKind, &str)] = &[
    (
        "true",
        UserCommandKind::True,
        "Usage: true\nReturn success without output.",
    ),
    (
        "false",
        UserCommandKind::False,
        "Usage: false\nReturn status 1 without output.",
    ),
    ("exit", UserCommandKind::Exit, exit::ExitArguments::HELP),
    (
        "cd",
        UserCommandKind::ChangeDirectory,
        change_directory::ChangeDirectoryArguments::HELP,
    ),
    (
        "echo",
        UserCommandKind::Echo,
        "Usage: echo [TEXT]...\nPrint text and expand environment variables.",
    ),
    ("ls", UserCommandKind::List, list::ListArguments::HELP),
    (
        "ln",
        UserCommandKind::Link,
        "Usage: ln TARGET LINK\nCreate a symbolic link.",
    ),
    (
        "readlink",
        UserCommandKind::ReadLink,
        "Usage: readlink LINK\nPrint a symbolic link target.",
    ),
    ("clear", UserCommandKind::Clear, clear::ClearArguments::HELP),
    (
        "cat",
        UserCommandKind::Concatenate,
        "Usage: cat [FILE]...\nPrint file contents.",
    ),
    (
        "grep",
        UserCommandKind::Grep,
        "Usage: grep PATTERN [FILE]...\nPrint lines containing the literal pattern; read standard input when no file is given or FILE is -.",
    ),
    (
        "stat",
        UserCommandKind::Statistics,
        statistics::StatisticsArguments::HELP,
    ),
    (
        "mkdir",
        UserCommandKind::CreateDirectory,
        directory::DirectoryCreateArguments::HELP,
    ),
    (
        "export",
        UserCommandKind::SetEnvironmentVariable,
        environment_variables::SetEnvironmentVariableArguments::HELP,
    ),
    (
        "unset",
        UserCommandKind::RemoveEnvironmentVariable,
        environment_variables::RemoveEnvironmentVariableArguments::HELP,
    ),
    (
        "rm",
        UserCommandKind::RemoveFile,
        directory::FileRemoveArguments::HELP,
    ),
    (
        "rmdir",
        UserCommandKind::RemoveDirectory,
        directory::DirectoryRemoveArguments::HELP,
    ),
    (
        "web_request",
        UserCommandKind::WebRequest,
        "Usage: web_request [-h HEADER] [-m METHOD] [-b BODY] [-f BODY_FILE] URL\nSend an HTTP request.",
    ),
    (
        "dns_resolve",
        UserCommandKind::DnsResolve,
        dns::DnsResolveArguments::HELP,
    ),
    ("ping", UserCommandKind::Ping, ping::PingArguments::HELP),
    ("ip", UserCommandKind::Ip, ip::IpArguments::HELP),
    (
        "pwd",
        UserCommandKind::PrintWorkingDirectory,
        "Usage: pwd\nPrint the current directory.",
    ),
    (
        "realpath",
        UserCommandKind::Realpath,
        realpath::RealpathArguments::HELP,
    ),
    (
        "printenv",
        UserCommandKind::PrintEnvironmentVariable,
        environment_variables::GetEnvironmentVariableArguments::HELP,
    ),
    ("which", UserCommandKind::Which, which::WhichArguments::HELP),
    (
        "wc",
        UserCommandKind::WordCount,
        word_count::WordCountArguments::HELP,
    ),
    ("head", UserCommandKind::Head, head::HeadArguments::HELP),
    ("tail", UserCommandKind::Tail, tail::TailArguments::HELP),
    ("help", UserCommandKind::Help, help::HelpArguments::HELP),
    ("tree", UserCommandKind::Tree, tree::TreeArguments::HELP),
    ("find", UserCommandKind::Find, find::FindArguments::HELP),
    ("mv", UserCommandKind::Move, mv::MoveArguments::HELP),
    ("cp", UserCommandKind::Copy, cp::CopyArguments::HELP),
    ("sleep", UserCommandKind::Sleep, sleep::SleepArguments::HELP),
    ("chmod", UserCommandKind::Chmod, chmod::ChmodArguments::HELP),
    (
        "touch",
        UserCommandKind::Touch,
        "Usage: touch FILE...\nCreate missing files and update access and modification times.",
    ),
];

pub fn resolve_user_command(name: &str) -> Option<UserCommandKind> {
    BUILTINS
        .iter()
        .find(|entry| entry.0 == name)
        .map(|entry| entry.1)
}

pub async fn dispatch_user_command<'a, I, C>(
    kind: UserCommandKind,
    context: &mut C,
    options: &mut getargs::Options<&'a str, I>,
    paths: &[&Path],
) -> Result<(), crate::Error<'a>>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let result = match kind {
        UserCommandKind::True => Ok(()),
        UserCommandKind::False => Err(crate::Error::ExitStatus(core::num::NonZeroUsize::MIN)),
        UserCommandKind::Exit => ExitCommand.execute(context, options, paths).await,
        UserCommandKind::ChangeDirectory => {
            ChangeDirectoryCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::Echo => EchoCommand.execute(context, options, paths).await,
        UserCommandKind::List => ListCommand.execute(context, options, paths).await,
        UserCommandKind::Clear => ClearCommand.execute(context, options, paths).await,
        UserCommandKind::Chmod => ChmodCommand.execute(context, options, paths).await,
        UserCommandKind::Concatenate => ConcatenateCommand.execute(context, options, paths).await,
        UserCommandKind::Grep => grep::GrepCommand.execute(context, options, paths).await,
        UserCommandKind::Statistics => StatisticsCommand.execute(context, options, paths).await,
        UserCommandKind::CreateDirectory => {
            CreateDirectoryCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::SetEnvironmentVariable => {
            SetEnvironmentVariableCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::RemoveEnvironmentVariable => {
            RemoveEnvironmentVariableCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::RemoveFile => RemoveFileCommand.execute(context, options, paths).await,
        UserCommandKind::RemoveDirectory => {
            RemoveDirectoryCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::WebRequest => WebRequestCommand.execute(context, options, paths).await,
        UserCommandKind::DnsResolve => DnsResolveCommand.execute(context, options, paths).await,
        UserCommandKind::Ping => PingCommand.execute(context, options, paths).await,
        UserCommandKind::Ip => IpCommand.execute(context, options, paths).await,
        UserCommandKind::PrintWorkingDirectory => {
            PrintWorkingDirectoryCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::Realpath => {
            realpath::RealpathCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::PrintEnvironmentVariable => {
            PrintEnvironmentVariableCommand
                .execute(context, options, paths)
                .await
        }
        UserCommandKind::Which => WhichCommand.execute(context, options, paths).await,
        UserCommandKind::WordCount => WordCountCommand.execute(context, options, paths).await,
        UserCommandKind::Head => HeadCommand.execute(context, options, paths).await,
        UserCommandKind::Tail => TailCommand.execute(context, options, paths).await,
        UserCommandKind::Touch => TouchCommand.execute(context, options, paths).await,
        UserCommandKind::Sleep => SleepCommand.execute(context, options, paths).await,
        UserCommandKind::Help => help::HelpCommand.execute(context, options, paths).await,
        UserCommandKind::Tree => TreeCommand.execute(context, options, paths).await,
        UserCommandKind::Find => FindCommand.execute(context, options, paths).await,
        UserCommandKind::Move => MoveCommand.execute(context, options, paths).await,
        UserCommandKind::Copy => CopyCommand.execute(context, options, paths).await,
        UserCommandKind::Link => ln::LinkCommand.execute(context, options, paths).await,
        UserCommandKind::ReadLink => ln::ReadLinkCommand.execute(context, options, paths).await,
    };

    match result {
        Err(crate::Error::ArgumentParsing(error)) if error.is_help() || error.is_version() => {
            context.write_out_fmt(format_args!("{error}\n")).await?;
            Ok(())
        }
        result => result,
    }
}

fn check_no_more_options<'a, I>(options: &mut getargs::Options<&'a str, I>) -> crate::Result<()>
where
    I: Iterator<Item = &'a str>,
{
    if Ok(None) != options.next_opt() {
        return Err(crate::Error::InvalidOption);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{UserCommandKind, resolve_user_command};

    #[derive(Default)]
    struct HelpContext {
        output: alloc::string::String,
        executable_context: Option<&'static xila::executable::ExecutableContext>,
        task: Option<xila::task::TaskIdentifier>,
    }

    impl super::CommandContext for HelpContext {
        fn executable_context(&self) -> &'static xila::executable::ExecutableContext {
            self.executable_context
                .expect("help must not access OS services")
        }
        fn task_id(&self) -> xila::task::TaskIdentifier {
            self.task.expect("help must not access the current task")
        }
        fn current_directory(&self) -> &xila::file_system::Path {
            panic!("help must not access the filesystem")
        }
        fn set_current_directory(&mut self, _: xila::file_system::PathOwned) {
            panic!("help must not change directory")
        }
        fn stop(&mut self) {
            panic!("exit --help must not stop the shell")
        }
        async fn write_out_fmt(
            &mut self,
            arguments: core::fmt::Arguments<'_>,
        ) -> crate::Result<()> {
            use core::fmt::Write;
            self.output.write_fmt(arguments).map_err(Into::into)
        }
        async fn write_out(&mut self, _: &[u8]) -> crate::Result<()> {
            panic!("unexpected byte output")
        }
        async fn write_out_line(&mut self, _: &[u8]) -> crate::Result<()> {
            panic!("unexpected line output")
        }
        fn standard(&mut self) -> &mut xila::executable::Standard<'static> {
            panic!("help must not write to stderr")
        }
    }

    #[test]
    fn builtin_help_is_successful_output_without_side_effects() {
        use core::{
            future::Future,
            task::{Context, Poll, Waker},
        };

        for (name, kind) in [
            ("head", UserCommandKind::Head),
            (
                "sleep",
                resolve_user_command("sleep").expect("sleep builtin"),
            ),
            ("tree", UserCommandKind::Tree),
            ("find", UserCommandKind::Find),
            ("mv", UserCommandKind::Move),
            ("exit", UserCommandKind::Exit),
            ("ping", UserCommandKind::Ping),
            ("ls", UserCommandKind::List),
            ("tail", UserCommandKind::Tail),
            ("wc", UserCommandKind::WordCount),
            ("cd", UserCommandKind::ChangeDirectory),
            ("clear", UserCommandKind::Clear),
            ("chmod", UserCommandKind::Chmod),
            ("realpath", UserCommandKind::Realpath),
            ("touch", UserCommandKind::Touch),
            ("mkdir", UserCommandKind::CreateDirectory),
            ("rm", UserCommandKind::RemoveFile),
            ("rmdir", UserCommandKind::RemoveDirectory),
            ("stat", UserCommandKind::Statistics),
            ("export", UserCommandKind::SetEnvironmentVariable),
            ("unset", UserCommandKind::RemoveEnvironmentVariable),
            ("printenv", UserCommandKind::PrintEnvironmentVariable),
            ("which", UserCommandKind::Which),
            ("dns_resolve", UserCommandKind::DnsResolve),
            ("ip", UserCommandKind::Ip),
        ] {
            let mut context = HelpContext {
                output: alloc::string::String::new(),
                ..Default::default()
            };
            let mut options = getargs::Options::new(["--help"].into_iter());
            {
                let mut future = core::pin::pin!(super::dispatch_user_command(
                    kind,
                    &mut context,
                    &mut options,
                    &[]
                ));
                let mut task_context = Context::from_waker(Waker::noop());
                assert!(matches!(
                    future.as_mut().poll(&mut task_context),
                    Poll::Ready(Ok(()))
                ));
            }
            assert!(context.output.contains(&alloc::format!("Usage: {name}")));
        }
    }

    #[xila::task::test(task_path = xila::task)]
    async fn sleep_yields_then_completes_without_accessing_os_services() {
        use core::{
            future::{Future, poll_fn},
            task::Poll,
        };
        let mut context = HelpContext {
            output: alloc::string::String::new(),
            ..Default::default()
        };
        let mut options = getargs::Options::new(["5ms"].into_iter());
        let mut future = core::pin::pin!(super::dispatch_user_command(
            resolve_user_command("sleep").unwrap(),
            &mut context,
            &mut options,
            &[]
        ));
        let start = std::time::Instant::now();
        poll_fn(|task_context| {
            assert!(matches!(future.as_mut().poll(task_context), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        future.await.unwrap();
        // Embassy truncates its clock to microseconds; allow for that rounding.
        assert!(start.elapsed() >= core::time::Duration::from_millis(4));
    }

    #[xila::task::test(task_path = xila::task)]
    async fn sleep_zero_completes_without_output() {
        let mut context = HelpContext {
            output: alloc::string::String::new(),
            ..Default::default()
        };
        let mut options = getargs::Options::new(["0"].into_iter());
        super::dispatch_user_command(
            resolve_user_command("sleep").unwrap(),
            &mut context,
            &mut options,
            &[],
        )
        .await
        .unwrap();
        assert!(context.output.is_empty());
    }

    #[test]
    fn sleep_rejects_invalid_duration_before_waiting() {
        use core::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        let mut context = HelpContext {
            output: alloc::string::String::new(),
            ..Default::default()
        };
        let mut options = getargs::Options::new(["1.5s"].into_iter());
        let mut future = core::pin::pin!(super::dispatch_user_command(
            resolve_user_command("sleep").unwrap(),
            &mut context,
            &mut options,
            &[]
        ));
        let mut task_context = Context::from_waker(Waker::noop());
        assert!(matches!(
            future.as_mut().poll(&mut task_context),
            Poll::Ready(Err(crate::Error::InvalidSleepDuration))
        ));
    }

    #[test]
    fn help_lists_builtins_and_shows_usage_without_running_commands() {
        use core::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        for (arguments, expected) in [
            (alloc::vec![], "Built-in commands:"),
            (alloc::vec!["exit"], "Usage: exit"),
            (alloc::vec!["cat"], "Usage: cat"),
            (alloc::vec!["echo"], "Usage: echo"),
            (alloc::vec!["pwd"], "Usage: pwd"),
            (alloc::vec!["web_request"], "Usage: web_request"),
            (alloc::vec!["help"], "Usage: help"),
            (alloc::vec!["--help"], "Usage: help"),
        ] {
            let mut context = HelpContext {
                output: alloc::string::String::new(),
                ..Default::default()
            };
            let mut options = getargs::Options::new(arguments.into_iter());
            {
                let mut future = core::pin::pin!(super::dispatch_user_command(
                    resolve_user_command("help").expect("help must be registered"),
                    &mut context,
                    &mut options,
                    &[]
                ));
                assert!(matches!(
                    future
                        .as_mut()
                        .poll(&mut Context::from_waker(Waker::noop())),
                    Poll::Ready(Ok(()))
                ));
            }
            assert!(context.output.contains(expected), "{}", context.output);
            if expected == "Built-in commands:" {
                for name in ["help", "exit", "ls", "wc", "tail", "grep"] {
                    assert!(context.output.lines().any(|line| line == name));
                }
            }
        }
    }

    #[test]
    fn help_rejects_unknown_commands_and_extra_arguments() {
        use core::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        for arguments in [["nonexistent", ""], ["ls", "extra"], ["--unknown", ""]] {
            let mut context = HelpContext {
                output: alloc::string::String::new(),
                ..Default::default()
            };
            let arguments = arguments
                .into_iter()
                .filter(|argument| !argument.is_empty());
            let mut options = getargs::Options::new(arguments);
            let mut future = core::pin::pin!(super::dispatch_user_command(
                resolve_user_command("help").expect("help must be registered"),
                &mut context,
                &mut options,
                &[]
            ));
            assert!(matches!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Err(_))
            ));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[xila::task::test(task_path = xila::task)]
    async fn help_discovers_mounted_executables_and_ignores_nonexecutable_entries() {
        use xila::{
            executable::mount_executables,
            file_system::{AccessFlags, CreateFlags, Flags, Path},
            virtual_file_system::{Directory, File},
        };
        let standard = testing::initialize(false, false).await;
        let executable_context = standard.context;
        let vfs = &executable_context.virtual_file_system;
        let task = executable_context
            .task_manager
            .get_current_task_identifier()
            .await;
        Directory::create(vfs, task, "/help_test").await.unwrap();
        Directory::create(vfs, task, "/help_test/subdirectory")
            .await
            .unwrap();
        let file = File::open(
            vfs,
            task,
            "/help_test/plain_file",
            Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
        )
        .await
        .unwrap();
        file.close(vfs).await.unwrap();
        mount_executables!(
            executable_context,
            vfs,
            task,
            &[("/help_test/tool", crate::ShellExecutable)]
        )
        .await
        .unwrap();
        let mut context = HelpContext {
            executable_context: Some(executable_context),
            task: Some(task),
            ..Default::default()
        };
        let paths = [
            Path::from_str("/help_missing"),
            Path::from_str("/help_test"),
        ];
        let mut options = getargs::Options::new([].into_iter());
        super::dispatch_user_command(
            resolve_user_command("help").unwrap(),
            &mut context,
            &mut options,
            &paths,
        )
        .await
        .unwrap();
        assert!(
            context.output.lines().any(|line| line == "tool"),
            "{}",
            context.output
        );
        assert!(!context.output.contains("plain_file"));
        assert!(!context.output.contains("subdirectory"));
        context.output.clear();
        let mut options = getargs::Options::new(["tool"].into_iter());
        super::dispatch_user_command(
            resolve_user_command("help").unwrap(),
            &mut context,
            &mut options,
            &paths,
        )
        .await
        .unwrap();
        assert!(context.output.contains("external executable"));
        for command in ["missing", "plain_file", "subdirectory"] {
            let mut options = getargs::Options::new([command].into_iter());
            assert!(matches!(
                super::dispatch_user_command(
                    resolve_user_command("help").unwrap(),
                    &mut context,
                    &mut options,
                    &paths
                )
                .await,
                Err(crate::Error::CommandNotFound)
            ));
        }
    }

    #[test]
    fn boolean_commands_resolve_without_os_services_or_output() {
        use core::{
            future::Future,
            task::{Context, Poll, Waker},
        };

        for (name, succeeds) in [("true", true), ("false", false)] {
            let kind = resolve_user_command(name).expect("boolean command must be a builtin");
            let mut context = HelpContext {
                output: alloc::string::String::new(),
                ..Default::default()
            };
            // Both commands ignore operands and options, as POSIX specifies.
            let mut options = getargs::Options::new(["--ignored", "operand"].into_iter());
            {
                let mut future = core::pin::pin!(super::dispatch_user_command(
                    kind,
                    &mut context,
                    &mut options,
                    &[]
                ));
                let mut task_context = Context::from_waker(Waker::noop());
                let Poll::Ready(result) = future.as_mut().poll(&mut task_context) else {
                    panic!("boolean commands must complete immediately");
                };
                assert_eq!(result.is_ok(), succeeds);
                if let Err(error) = result {
                    assert_eq!(core::num::NonZeroUsize::from(error).get(), 1);
                }
            }
            assert!(context.output.is_empty());
        }
    }

    #[test]
    fn resolves_all_supported_user_commands() {
        assert!(matches!(
            resolve_user_command("exit"),
            Some(UserCommandKind::Exit)
        ));
        assert!(matches!(
            resolve_user_command("cd"),
            Some(UserCommandKind::ChangeDirectory)
        ));
        assert!(matches!(
            resolve_user_command("echo"),
            Some(UserCommandKind::Echo)
        ));
        assert!(matches!(
            resolve_user_command("ls"),
            Some(UserCommandKind::List)
        ));
        assert!(matches!(
            resolve_user_command("clear"),
            Some(UserCommandKind::Clear)
        ));
        assert!(matches!(
            resolve_user_command("cat"),
            Some(UserCommandKind::Concatenate)
        ));
        assert!(matches!(
            resolve_user_command("grep"),
            Some(UserCommandKind::Grep)
        ));
        assert!(matches!(
            resolve_user_command("stat"),
            Some(UserCommandKind::Statistics)
        ));
        assert!(matches!(
            resolve_user_command("mkdir"),
            Some(UserCommandKind::CreateDirectory)
        ));
        assert!(matches!(
            resolve_user_command("export"),
            Some(UserCommandKind::SetEnvironmentVariable)
        ));
        assert!(matches!(
            resolve_user_command("unset"),
            Some(UserCommandKind::RemoveEnvironmentVariable)
        ));
        assert!(matches!(
            resolve_user_command("rm"),
            Some(UserCommandKind::RemoveFile)
        ));
        assert!(matches!(
            resolve_user_command("rmdir"),
            Some(UserCommandKind::RemoveDirectory)
        ));
        assert!(matches!(
            resolve_user_command("web_request"),
            Some(UserCommandKind::WebRequest)
        ));
        assert!(matches!(
            resolve_user_command("dns_resolve"),
            Some(UserCommandKind::DnsResolve)
        ));
        assert!(matches!(
            resolve_user_command("ping"),
            Some(UserCommandKind::Ping)
        ));
        assert!(matches!(
            resolve_user_command("ip"),
            Some(UserCommandKind::Ip)
        ));
        assert!(matches!(
            resolve_user_command("pwd"),
            Some(UserCommandKind::PrintWorkingDirectory)
        ));
        assert!(matches!(
            resolve_user_command("printenv"),
            Some(UserCommandKind::PrintEnvironmentVariable)
        ));
        assert!(matches!(
            resolve_user_command("which"),
            Some(UserCommandKind::Which)
        ));
        assert!(matches!(
            resolve_user_command("wc"),
            Some(UserCommandKind::WordCount)
        ));
        assert!(matches!(
            resolve_user_command("head"),
            Some(UserCommandKind::Head)
        ));
        assert!(matches!(
            resolve_user_command("tail"),
            Some(UserCommandKind::Tail)
        ));
        assert!(matches!(
            resolve_user_command("chmod"),
            Some(UserCommandKind::Chmod)
        ));
        assert!(matches!(
            resolve_user_command("touch"),
            Some(UserCommandKind::Touch)
        ));
        assert!(matches!(
            resolve_user_command("tree"),
            Some(UserCommandKind::Tree)
        ));
        assert!(matches!(
            resolve_user_command("realpath"),
            Some(UserCommandKind::Realpath)
        ));
        assert!(matches!(
            resolve_user_command("mv"),
            Some(UserCommandKind::Move)
        ));
        assert!(resolve_user_command("unknown").is_none());
    }
}
