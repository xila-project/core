use super::*;
use alloc::{format, vec};
use core::num::NonZeroUsize;
use xila::executable::{ExecutableTrait, MainFuture, mount_executables};

async fn shell() -> Shell {
    let mut standard = testing::initialize(false, false).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;
    standard.standard_in = File::open(
        context.virtual_file_system,
        task,
        "/devices/standard_io/null",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();
    standard.standard_out = File::open(
        context.virtual_file_system,
        task,
        "/shell-output",
        Flags::new(
            AccessFlags::READ_WRITE,
            Some(CreateFlags::CREATE_TRUNCATE),
            None,
        ),
    )
    .await
    .unwrap();
    standard.standard_error = File::open(
        context.virtual_file_system,
        task,
        "/shell-errors",
        Flags::new(
            AccessFlags::READ_WRITE,
            Some(CreateFlags::CREATE_TRUNCATE),
            None,
        ),
    )
    .await
    .unwrap();
    Shell::new(standard).await
}
async fn contents(shell: &Shell, path: &str) -> String {
    let mut output = Vec::new();
    File::read_from_path(
        shell.context.virtual_file_system,
        shell.task,
        path,
        &mut output,
    )
    .await
    .unwrap();
    String::from_utf8(output).unwrap()
}

async fn write_file(shell: &Shell, path: &str, bytes: &[u8]) {
    let mut file = File::open(
        shell.context.virtual_file_system,
        shell.task,
        path,
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
    )
    .await
    .unwrap();
    write_all(&mut file, bytes).await.unwrap();
    file.close(shell.context.virtual_file_system).await.unwrap();
}

#[xila::task::test(task_path = xila::task)]
async fn launched_scripts_inherit_the_callers_directory() {
    let mut shell = shell().await;
    mount_executables!(
        shell.context,
        shell.context.virtual_file_system,
        shell.task,
        &[(&"/applications/executables/shell", crate::ShellExecutable)]
    )
    .await
    .unwrap();
    write_file(
        &shell,
        "/temporary/job",
        b"pwd\necho child > marker\ncd /\n",
    )
    .await;
    assert_eq!(shell.execute_line("cd /temporary").await.unwrap(), 0);

    for path in ["job", "/temporary/job"] {
        assert_eq!(
            shell
                .execute_line(&format!("/applications/executables/shell -s {path}"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(contents(&shell, "/temporary/marker").await, "child\n");
        assert_eq!(shell.current_directory.as_str(), "/temporary");
    }
    assert_eq!(
        contents(&shell, "/shell-output").await,
        "/temporary\n/temporary\n"
    );
}

#[xila::task::test(task_path = xila::task)]
async fn sequential_shell_invocations_do_not_accumulate_nesting_depth() {
    let mut shell = shell().await;
    for _ in 0..6 {
        assert_eq!(shell.main(vec!["true".into()]).await, Ok(()));
    }
    assert!(
        shell
            .context
            .task_manager
            .get_environment_variable(shell.task, crate::SCRIPT_DEPTH_VARIABLE)
            .await
            .is_err()
    );
}

#[xila::task::test(task_path = xila::task)]
async fn shell_invocations_restore_inherited_depth_after_errors() {
    let mut shell = shell().await;
    shell
        .context
        .task_manager
        .set_environment_variable(shell.task, crate::SCRIPT_DEPTH_VARIABLE, "1")
        .await
        .unwrap();
    for arguments in [
        vec!["-s".into(), "/missing".into()],
        vec!["-c".into(), "echo '".into()],
    ] {
        assert!(shell.main(arguments).await.is_err());
        assert_eq!(
            shell
                .context
                .task_manager
                .get_environment_variable(shell.task, crate::SCRIPT_DEPTH_VARIABLE)
                .await
                .unwrap()
                .get_value(),
            "1"
        );
    }
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_streams_commands_and_keeps_shell_state_without_consuming_stdin() {
    let mut shell = shell().await;
    write_file(
        &shell,
        "/script",
        b"\n# comment\nexport NAME='hello world'\ncd /temporary\necho \"$NAME\"\ncat\n",
    )
    .await;

    assert_eq!(
        shell.main(vec!["-s".into(), "/script".into()]).await,
        Ok(())
    );
    assert_eq!(contents(&shell, "/shell-output").await, "hello world\n");
    assert_eq!(shell.current_directory.as_str(), "/temporary");
    assert_eq!(
        shell
            .context
            .task_manager
            .get_environment_variable(shell.task, "NAME")
            .await
            .unwrap()
            .get_value(),
        "hello world"
    );
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_handles_crlf_final_line_and_hash_quoting() {
    let mut shell = shell().await;
    write_file(
        &shell,
        "/script",
        b"echo \"a#b\" 'c#d' e\\#f # trailing comment\r\necho final",
    )
    .await;

    assert_eq!(
        shell.main(vec!["-s".into(), "/script".into()]).await,
        Ok(())
    );
    assert_eq!(
        contents(&shell, "/shell-output").await,
        "a#b c#d e#f\nfinal\n"
    );
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_continues_after_command_failure_and_returns_last_status() {
    let mut shell = shell().await;
    write_file(&shell, "/script", b"false\necho continued\nfalse").await;

    assert_eq!(
        shell
            .main(vec!["-s".into(), "/script".into()])
            .await
            .unwrap_err()
            .get(),
        1
    );
    assert_eq!(contents(&shell, "/shell-output").await, "continued\n");
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_reports_line_errors_stops_and_closes_the_file() {
    let mut shell = shell().await;
    write_file(
        &shell,
        "/script",
        b"echo first\necho 'unterminated\necho wrong\n",
    )
    .await;

    assert_eq!(
        shell
            .main(vec!["-s".into(), "/script".into()])
            .await
            .unwrap_err()
            .get(),
        2
    );
    assert_eq!(contents(&shell, "/shell-output").await, "first\n");
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains("/script:2:")
    );
    assert_eq!(shell.execute_line("mv /script /renamed").await.unwrap(), 0);
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_reports_missing_and_oversized_files_with_nonzero_status() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .main(vec!["-s".into(), "/missing-script".into()])
            .await
            .unwrap_err()
            .get(),
        1
    );
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains("/missing-script")
    );

    let mut maximum_line = vec![b'#'; crate::tokenizer::MAX_INPUT_BYTES];
    maximum_line.extend_from_slice(b"\r\n");
    write_file(&shell, "/script", &maximum_line).await;
    assert_eq!(
        shell.main(vec!["-s".into(), "/script".into()]).await,
        Ok(())
    );

    write_file(
        &shell,
        "/script",
        &vec![b'a'; crate::tokenizer::MAX_INPUT_BYTES + 1],
    )
    .await;
    assert_eq!(
        shell
            .main(vec!["-s".into(), "/script".into()])
            .await
            .unwrap_err()
            .get(),
        2
    );
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains("/script:1:")
    );

    let mut unreadable = File::open(
        shell.context.virtual_file_system,
        shell.task,
        "/unreadable-script",
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
    )
    .await
    .unwrap();
    write_all(&mut unreadable, b"echo hidden\n").await.unwrap();
    unreadable
        .set_permissions(xila::file_system::Permissions::from_octal(0).unwrap())
        .await
        .unwrap();
    drop(unreadable);
    let administrator = shell
        .context
        .users_manager
        .get_user_identifier("administrator")
        .await
        .unwrap();
    shell
        .context
        .task_manager
        .set_user(shell.task, administrator)
        .await
        .unwrap();
    assert_eq!(
        shell
            .main(vec!["-s".into(), "/unreadable-script".into()])
            .await
            .unwrap_err()
            .get(),
        1
    );
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains("/unreadable-script")
    );
}

#[xila::task::test(task_path = xila::task)]
async fn quoting_expansion_and_short_circuit_keep_shell_state() {
    let mut shell = shell().await;
    assert_eq!(shell.execute_line("export NAME='hello world'; echo \"$NAME\" '$NAME' \\$NAME; false && echo wrong || echo fallback; true || echo wrong; echo $?").await.unwrap(), 0);
    assert_eq!(
        contents(&shell, "/shell-output").await,
        "hello world $NAME $NAME\nfallback\n0\n"
    );
    assert_eq!(
        shell
            .execute_line("cd /temporary && pwd; cd / | cat; pwd")
            .await
            .unwrap(),
        0
    );
    assert!(
        contents(&shell, "/shell-output")
            .await
            .ends_with("/temporary\n/temporary\n")
    );
    assert_eq!(shell.execute_line("false").await.unwrap(), 1);
    assert_eq!(shell.execute_line("missing-command").await.unwrap(), 127);
}

#[xila::task::test(task_path = xila::task)]
async fn redirection_appends_truncates_and_reads_without_extra_newlines() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .execute_line("echo first>/one; echo second>>/one; cat</one>/copy")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/copy").await, "first\nsecond\n");
    assert_eq!(
        shell.execute_line("echo x>/one; cat /one").await.unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/one").await, "x\n");
    assert_eq!(contents(&shell, "/shell-output").await, "x\n");
    assert_eq!(
        shell
            .execute_line("cat /missing 2>/errors || echo recovered")
            .await
            .unwrap(),
        0
    );
    assert!(!contents(&shell, "/errors").await.is_empty());
    assert!(
        contents(&shell, "/shell-output")
            .await
            .ends_with("recovered\n")
    );
}

#[xila::task::test(task_path = xila::task)]
async fn grep_streams_matching_lines_and_reports_status_and_line_errors() {
    let mut shell = shell().await;
    File::write_to_path(
        shell.context.virtual_file_system,
        shell.task,
        "/grep-input",
        b"miss\nhit one\nno match\nhit eof",
    )
    .await
    .unwrap();

    assert_eq!(
        shell
            .execute_line("grep hit /grep-input > /grep-output")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/grep-output").await, "hit one\nhit eof");
    assert_eq!(
        shell.execute_line("grep absent /grep-input").await.unwrap(),
        1
    );

    assert_eq!(
        shell
            .execute_line("echo 'pipeline hit'|grep hit|cat>/grep-pipe")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/grep-pipe").await, "pipeline hit\n");
    assert_eq!(
        shell
            .execute_line("echo 'dash hit'|grep hit -|cat>/grep-dash")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/grep-dash").await, "dash hit\n");

    let long_line = vec![b'x'; 257];
    File::write_to_path(
        shell.context.virtual_file_system,
        shell.task,
        "/grep-long-line",
        &long_line,
    )
    .await
    .unwrap();
    assert_eq!(
        shell.execute_line("grep x /grep-long-line").await.unwrap(),
        1
    );
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains(xila::internationalization::translate!("Line too long"))
    );
}

struct ReadOne;
impl ExecutableTrait for ReadOne {
    fn main(mut standard: Standard<'static>, _: Vec<String>) -> MainFuture {
        alloc::boxed::Box::pin(async move {
            standard.input().read(&mut [0; 1]).await.unwrap();
            Ok(())
        })
    }
}
struct Fail;
impl ExecutableTrait for Fail {
    fn main(_: Standard<'static>, _: Vec<String>) -> MainFuture {
        alloc::boxed::Box::pin(async { Err(NonZeroUsize::new(7).unwrap()) })
    }
}

#[xila::task::test(task_path = xila::task)]
async fn bounded_pipelines_stream_large_output_and_close_early_consumers() {
    let mut shell = shell().await;
    let context = shell.context;
    let task = shell.task;
    let virtual_file_system = context.virtual_file_system;
    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[
            (&"/applications/executables/read-one", ReadOne),
            (&"/applications/executables/fail", Fail)
        ]
    )
    .await
    .unwrap();
    let payload = "x".repeat(3000);
    assert_eq!(
        shell
            .execute_line(&format!("echo '{payload}'|cat|wc -c>/count"))
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/count").await.trim(), "3001");
    assert_eq!(
        shell
            .execute_line(&format!(
                "echo '{payload}'|/applications/executables/read-one"
            ))
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        shell
            .execute_line("/applications/executables/fail && echo wrong || echo correct")
            .await
            .unwrap(),
        0
    );
    assert!(
        contents(&shell, "/shell-output")
            .await
            .ends_with("correct\n")
    );
    assert_eq!(
        shell
            .execute_line("true|/applications/executables/fail")
            .await
            .unwrap(),
        7
    );
    assert_eq!(
        shell
            .execute_line("/applications/executables/fail|true")
            .await
            .unwrap(),
        0
    );
}

#[xila::task::test(task_path = xila::task)]
async fn malformed_source_does_not_execute_a_valid_prefix() {
    let mut shell = shell().await;
    assert!(
        shell
            .execute_line("echo changed>/must-not-exist; echo 'unfinished")
            .await
            .is_err()
    );
    assert!(
        File::open(
            shell.context.virtual_file_system,
            shell.task,
            "/must-not-exist",
            AccessFlags::Read.into()
        )
        .await
        .is_err()
    );
    assert_eq!(shell.execute_line("exit; echo wrong").await.unwrap(), 0);
    assert_eq!(contents(&shell, "/shell-output").await, "");
}

struct Delayed;
impl ExecutableTrait for Delayed {
    fn main(mut standard: Standard<'static>, _: Vec<String>) -> MainFuture {
        alloc::boxed::Box::pin(async move {
            assert_eq!(standard.input().read(&mut [0; 1]).await.unwrap(), 0);
            xila::task::sleep(core::time::Duration::from_millis(20)).await;
            write_all(standard.out(), b"background\n").await.unwrap();
            Ok(())
        })
    }
}
#[xila::task::test(task_path = xila::task)]
async fn background_jobs_release_the_prompt_and_have_independent_input() {
    let mut shell = shell().await;
    let context = shell.context;
    let task = shell.task;
    let virtual_file_system = context.virtual_file_system;
    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[(&"/applications/executables/delayed", Delayed)]
    )
    .await
    .unwrap();
    assert_eq!(
        shell
            .execute_line(
                "/applications/executables/delayed >/background & echo foreground>/foreground"
            )
            .await
            .unwrap(),
        0
    );
    assert!(!shell.background_jobs[0].is_finished());
    assert_eq!(contents(&shell, "/foreground").await, "foreground\n");
    for job in shell.background_jobs.drain(..) {
        assert_eq!(job.join().await, 0);
    }
    assert_eq!(contents(&shell, "/background").await, "background\n");
}

#[xila::task::test(task_path = xila::task)]
async fn literal_arguments_are_not_reparsed_and_command_mode_returns_failure() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .run_arguments(&["echo".into(), "$literal; not syntax".into()])
            .await,
        0
    );
    shell.standard.out().flush().await.unwrap();
    assert_eq!(
        contents(&shell, "/shell-output").await,
        "$literal; not syntax\n"
    );
    assert_eq!(
        shell
            .main(vec!["-c".into(), "false".into()])
            .await
            .unwrap_err()
            .get(),
        1
    );
}

#[xila::task::test(task_path = xila::task)]
async fn redirection_checks_existing_file_permissions_before_truncating() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .execute_line("echo original>/protected")
            .await
            .unwrap(),
        0
    );
    let mut file = File::open(
        shell.context.virtual_file_system,
        shell.task,
        "/protected",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();
    file.set_permissions(xila::file_system::Permissions::from_octal(0o444).unwrap())
        .await
        .unwrap();
    drop(file);
    let administrator = shell
        .context
        .users_manager
        .get_user_identifier("administrator")
        .await
        .unwrap();
    shell
        .context
        .task_manager
        .set_user(shell.task, administrator)
        .await
        .unwrap();
    assert_eq!(
        shell.execute_line("echo changed>/protected").await.unwrap(),
        1
    );
    assert_eq!(contents(&shell, "/protected").await, "original\n");
    assert_eq!(
        shell
            .execute_line("echo changed>/forbidden-new-file")
            .await
            .unwrap(),
        1
    );
    assert!(
        File::open(
            shell.context.virtual_file_system,
            shell.task,
            "/forbidden-new-file",
            AccessFlags::Read.into()
        )
        .await
        .is_err()
    );
}

#[xila::task::test(task_path = xila::task)]
async fn duplicated_pipe_writers_must_all_close_before_eof() {
    let shell = shell().await;
    let (reader, writer) = File::create_unnamed_pipe(
        shell.context.virtual_file_system,
        8,
        StateFlags::from_bits_truncate(0),
    )
    .await
    .unwrap();
    let mut reader = reader;
    let duplicate = writer.duplicate().await.unwrap();
    writer
        .close(shell.context.virtual_file_system)
        .await
        .unwrap();
    duplicate
        .close(shell.context.virtual_file_system)
        .await
        .unwrap();
    assert_eq!(reader.read(&mut [0; 1]).await.unwrap(), 0);
    reader
        .close(shell.context.virtual_file_system)
        .await
        .unwrap();
}

#[xila::task::test(task_path = xila::task)]
async fn duplicated_regular_streams_share_their_position() {
    let mut shell = shell().await;
    let mut duplicate = shell.standard.out().duplicate().await.unwrap();
    write_all(shell.standard.out(), b"a").await.unwrap();
    write_all(&mut duplicate, b"b").await.unwrap();
    write_all(shell.standard.out(), b"c").await.unwrap();
    duplicate.flush().await.unwrap();
    drop(duplicate);
    assert_eq!(contents(&shell, "/shell-output").await, "abc");
}

struct FailingFlush;
impl xila::file_system::DirectBaseOperations for FailingFlush {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: xila::file_system::Size,
    ) -> core::task::Poll<xila::file_system::Result<usize>> {
        xila::file_system::ready(|| Ok(0))
    }
    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        bytes: &[u8],
        _: xila::file_system::Size,
    ) -> core::task::Poll<xila::file_system::Result<usize>> {
        xila::file_system::ready(|| Ok(bytes.len()))
    }
    fn flush(
        &self,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<xila::file_system::Result<()>> {
        xila::file_system::ready(|| Err(xila::file_system::Error::InputOutput))
    }
}
impl xila::file_system::MountOperations for FailingFlush {}
impl xila::file_system::CharacterDevice for FailingFlush {}

struct FailingRead;
impl xila::file_system::DirectBaseOperations for FailingRead {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &mut [u8],
        _: xila::file_system::Size,
    ) -> core::task::Poll<xila::file_system::Result<usize>> {
        xila::file_system::ready(|| Err(xila::file_system::Error::InputOutput))
    }
    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        bytes: &[u8],
        _: xila::file_system::Size,
    ) -> core::task::Poll<xila::file_system::Result<usize>> {
        xila::file_system::ready(|| Ok(bytes.len()))
    }
    fn flush(
        &self,
        _cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<xila::file_system::Result<()>> {
        xila::file_system::ready(|| Ok(()))
    }
}
impl xila::file_system::MountOperations for FailingRead {
    fn unmount(&self) -> xila::file_system::Result<()> {
        Ok(())
    }
}
impl xila::file_system::CharacterDevice for FailingRead {}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_reports_read_errors_and_closes_the_script_file() {
    let mut shell = shell().await;
    shell
        .context
        .virtual_file_system
        .mount_character_device(shell.task, "/devices/failing-read", FailingRead)
        .await
        .unwrap();

    assert_eq!(
        shell
            .main(vec!["-s".into(), "/devices/failing-read".into()])
            .await
            .unwrap_err()
            .get(),
        2
    );
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains("/devices/failing-read:1:")
    );
    shell
        .context
        .virtual_file_system
        .unmount(&"/devices/failing-read", false)
        .await
        .unwrap();
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_rejects_shell_nesting_beyond_the_limit() {
    let mut shell = shell().await;
    shell
        .context
        .task_manager
        .set_environment_variable(
            shell.task,
            crate::SCRIPT_DEPTH_VARIABLE,
            &crate::MAX_SHELL_DEPTH.to_string(),
        )
        .await
        .unwrap();

    assert_eq!(
        shell
            .main(vec!["-s".into(), "/script".into()])
            .await
            .unwrap_err()
            .get(),
        2
    );
    assert!(
        contents(&shell, "/shell-errors")
            .await
            .contains("/script: Maximum nested shell depth exceeded")
    );
}

#[xila::task::test(task_path = xila::task)]
async fn script_mode_exit_stops_execution_and_closes_the_file() {
    let mut shell = shell().await;
    write_file(&shell, "/script", b"exit\necho wrong\n").await;

    assert_eq!(
        shell.main(vec!["-s".into(), "/script".into()]).await,
        Ok(())
    );
    assert_eq!(contents(&shell, "/shell-output").await, "");
    shell
        .context
        .virtual_file_system
        .rename(shell.task, &"/script", &"/renamed-script")
        .await
        .unwrap();
}

#[xila::task::test(task_path = xila::task)]
async fn pipeline_flush_failures_control_conditional_execution() {
    let mut shell = shell().await;
    shell
        .context
        .virtual_file_system
        .mount_character_device(shell.task, "/devices/failing-flush", FailingFlush)
        .await
        .unwrap();
    assert_eq!(
        shell
            .execute_line("echo value|cat>/devices/failing-flush && echo wrong")
            .await
            .unwrap(),
        1
    );
    assert_eq!(contents(&shell, "/shell-output").await, "");
}

#[xila::task::test(task_path = xila::task)]
async fn expansion_is_bounded_and_does_not_reinterpret_variable_contents() {
    let mut shell = shell().await;
    shell
        .context
        .task_manager
        .set_environment_variable(shell.task, "VALUE", "one two; echo wrong")
        .await
        .unwrap();
    assert_eq!(
        shell
            .execute_line("echo ${VALUE} '$VALUE' \"$MISSING\"")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        contents(&shell, "/shell-output").await,
        "one two; echo wrong $VALUE \n"
    );
    shell
        .context
        .task_manager
        .set_environment_variable(shell.task, "VALUE", &"x".repeat(MAX_EXPANDED_BYTES))
        .await
        .unwrap();
    assert_eq!(shell.execute_line("echo $VALUE").await.unwrap(), 1);
}

#[xila::task::test(task_path = xila::task)]
async fn duplicated_device_keeps_its_mount_busy() {
    let shell = shell().await;
    let filesystem = shell.context.virtual_file_system;
    filesystem
        .mount_character_device(shell.task, "/devices/duplicate", drivers_core::NullDevice)
        .await
        .unwrap();
    let original = File::open(
        filesystem,
        shell.task,
        "/devices/duplicate",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();
    let duplicate = original.duplicate().await.unwrap();
    drop(original);
    assert_eq!(
        filesystem.unmount(&"/devices/duplicate", false).await,
        Err(xila::virtual_file_system::Error::RessourceBusy)
    );
    drop(duplicate);
}

#[xila::task::test(task_path = xila::task)]
async fn interactive_eof_executes_the_final_line_once() {
    let mut shell = shell().await;
    shell
        .context
        .task_manager
        .set_environment_variable(shell.task, "User", "root")
        .await
        .unwrap();
    // Create input directly so EOF, rather than a trailing newline, ends the command.
    let mut input = File::open(
        shell.context.virtual_file_system,
        shell.task,
        "/shell-input",
        Flags::new(
            AccessFlags::READ_WRITE,
            Some(CreateFlags::CREATE_TRUNCATE),
            None,
        ),
    )
    .await
    .unwrap();
    write_all(&mut input, b"echo final").await.unwrap();
    input
        .set_position(&xila::file_system::Position::Start(0))
        .await
        .unwrap();
    shell.standard.standard_in = input;
    shell.main(Vec::new()).await.unwrap();
    let output = contents(&shell, "/shell-output").await;
    assert_eq!(output.matches("final\n").count(), 1);
}

#[xila::task::test(task_path = xila::task)]
async fn utility_commands_compose_with_streaming_and_redirection() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .execute_line("sleep 0 && help sleep | cat > /sleep-help")
            .await
            .unwrap(),
        0
    );
    assert!(
        contents(&shell, "/sleep-help")
            .await
            .contains("Usage: sleep")
    );
    assert_eq!(shell.execute_line("mkdir /utility-tree; echo saved > /utility-tree/file; chmod 600 /utility-tree/file; tree -L 1 /utility-tree | cat > /tree-output").await.unwrap(), 0);
    assert_eq!(
        contents(&shell, "/tree-output").await,
        "/utility-tree\n|-- file\n"
    );
    assert_eq!(shell.execute_line("mkdir /utility-tree/sub; echo found > /utility-tree/sub/target.txt; find /utility-tree --name target.txt --type f > /find-output").await.unwrap(), 0);
    assert_eq!(
        contents(&shell, "/find-output").await,
        "/utility-tree/sub/target.txt\n"
    );
    assert_eq!(
        shell
            .context
            .virtual_file_system
            .get_statistics(&"/utility-tree/file")
            .await
            .unwrap()
            .permissions
            .as_u16(),
        0o600
    );
    assert_eq!(
        shell
            .execute_line("false 2> /false-errors || true")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/false-errors").await, "");
}

#[xila::task::test(task_path = xila::task)]
async fn realpath_resolves_relative_and_multiple_existing_operands() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .execute_line("mkdir /realpath; echo value > /realpath/file; cd /realpath; realpath . file /realpath/../realpath/file")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        contents(&shell, "/shell-output").await,
        "/realpath\n/realpath/file\n/realpath/file\n"
    );
    assert_eq!(shell.execute_line("help realpath").await.unwrap(), 0);
    assert!(
        contents(&shell, "/shell-output")
            .await
            .contains("Usage: realpath PATH...")
    );
}

#[xila::task::test(task_path = xila::task)]
async fn realpath_rejects_missing_operands() {
    let mut shell = shell().await;
    assert_eq!(shell.execute_line("realpath /missing").await.unwrap(), 1);
    assert_eq!(contents(&shell, "/shell-output").await, "");
}

#[xila::task::test(task_path = xila::task)]
async fn mv_renames_a_file_to_a_new_path() {
    let mut shell = shell().await;
    assert_eq!(
        shell
            .execute_line("echo moved > /mv-source; mv /mv-source /mv-destination")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/mv-destination").await, "moved\n");
    assert!(
        shell
            .context
            .virtual_file_system
            .get_statistics(&"/mv-source")
            .await
            .is_err()
    );
}

#[xila::task::test(task_path = xila::task)]
async fn mv_rejects_invalid_operands_and_reports_rename_errors() {
    let mut shell = shell().await;
    assert_eq!(shell.execute_line("mv").await.unwrap(), 1);
    assert_eq!(shell.execute_line("mv source").await.unwrap(), 1);
    assert_eq!(
        shell
            .execute_line("mv source destination extra")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        shell
            .execute_line("mv -f source destination")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        shell
            .execute_line("echo source > /mv-error-source; echo destination > /mv-error-destination; mv /missing /new")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        shell
            .execute_line("mv /mv-error-source /mv-error-destination")
            .await
            .unwrap(),
        0
    );
    assert_eq!(contents(&shell, "/mv-error-destination").await, "source\n");
    assert!(
        shell
            .context
            .virtual_file_system
            .get_statistics(&"/mv-error-source")
            .await
            .is_err()
    );
}

#[xila::task::test(task_path = xila::task)]
async fn mv_checks_write_permission_on_both_parent_directories() {
    use xila::users::{GroupIdentifier, UserIdentifier};

    let mut shell = shell().await;
    assert_eq!(
        shell
            .execute_line("mkdir /mv-protected; echo protected > /mv-protected/source")
            .await
            .unwrap(),
        0
    );

    let owner = UserIdentifier::new(265);
    let other = UserIdentifier::new(266);
    for (user, name) in [(owner, "mv_owner"), (other, "mv_other")] {
        shell
            .context
            .users_manager
            .add_user(user, name, GroupIdentifier::ROOT)
            .await
            .unwrap();
    }

    let vfs = shell.context.virtual_file_system;
    vfs.set_ownership(shell.task, "/mv-protected", Some(owner), None)
        .await
        .unwrap();
    vfs.set_permissions(
        shell.task,
        "/mv-protected",
        xila::file_system::Permissions::from_octal(0o700).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        shell
            .execute_line("mkdir /mv-public; echo public > /mv-public/source")
            .await
            .unwrap(),
        0
    );
    vfs.set_ownership(shell.task, "/mv-public", Some(other), None)
        .await
        .unwrap();
    vfs.set_permissions(
        shell.task,
        "/mv-public",
        xila::file_system::Permissions::from_octal(0o700).unwrap(),
    )
    .await
    .unwrap();

    shell
        .context
        .task_manager
        .set_user(shell.task, other)
        .await
        .unwrap();
    assert_eq!(
        shell
            .execute_line("mv /mv-protected/source /mv-protected/denied")
            .await
            .unwrap(),
        1
    );
    // The rejected operation must leave the source in place. Check it as its
    // owner because the current user cannot traverse this directory.
    shell
        .context
        .task_manager
        .set_user(shell.task, owner)
        .await
        .unwrap();
    assert!(vfs.get_statistics(&"/mv-protected/source").await.is_ok());
    shell
        .context
        .task_manager
        .set_user(shell.task, other)
        .await
        .unwrap();
    assert_eq!(
        shell
            .execute_line("mv /mv-public/source /mv-protected/denied")
            .await
            .unwrap(),
        1
    );
    assert!(vfs.get_statistics(&"/mv-public/source").await.is_ok());

    shell
        .context
        .task_manager
        .set_user(shell.task, owner)
        .await
        .unwrap();
    assert_eq!(
        shell
            .execute_line("mv /mv-protected/source /mv-protected/allowed")
            .await
            .unwrap(),
        0
    );
    assert!(vfs.get_statistics(&"/mv-protected/source").await.is_err());
}

#[xila::task::test(task_path = xila::task)]
async fn rename_checks_parent_paths_relative_to_the_mounted_filesystem() {
    let shell = shell().await;
    let device = xila::file_system::MemoryDevice::<512>::new_static(1024 * 512);
    little_fs::FileSystem::format(device, 256).unwrap();
    let file_system = little_fs::FileSystem::new(device, 256).unwrap();
    xila::file_system::FileSystemOperations::set_attributes(
        &file_system,
        xila::file_system::Path::ROOT,
        &xila::file_system::Attributes::new()
            .set_user(xila::users::UserIdentifier::ROOT)
            .set_group(xila::users::GroupIdentifier::ROOT)
            .set_permissions(xila::file_system::Permissions::ALL_FULL),
    )
    .unwrap();
    let source_path = xila::file_system::Path::from_str("/source");
    xila::file_system::FileSystemOperations::create_file(&file_system, source_path).unwrap();
    xila::file_system::FileSystemOperations::set_attributes(
        &file_system,
        source_path,
        &xila::file_system::Attributes::new()
            .set_user(xila::users::UserIdentifier::ROOT)
            .set_group(xila::users::GroupIdentifier::ROOT)
            .set_permissions(xila::file_system::Permissions::FILE_DEFAULT)
            .set_kind(xila::file_system::Kind::File),
    )
    .unwrap();

    let vfs = shell.context.virtual_file_system;
    vfs.mount_file_system(file_system, "/mv-nested-mount", shell.task)
        .await
        .unwrap();

    vfs.rename(
        shell.task,
        &"/mv-nested-mount/source",
        &"/mv-nested-mount/destination",
    )
    .await
    .unwrap();
}
