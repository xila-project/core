#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[ignore]
#[xila::task::test(task_path = xila::task)]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate alloc;

    use xila::executable::{self, mount_executables};

    let standard = testing::initialize(false, true).await;
    let context = standard.context;
    let virtual_file_system = &context.virtual_file_system;
    let task = context.task_manager.get_current_task_identifier().await;

    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[(
            &"/applications/executables/command_line_shell",
            command_line_shell::ShellExecutable
        ),]
    )
    .await
    .unwrap();

    let result = executable::execute(
        context,
        "/applications/executables/command_line_shell",
        vec![],
        standard,
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    assert!(result == 0);
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[xila::task::test(task_path = xila::task)]
async fn boolean_command_status_and_interactive_continuation() {
    use xila::{
        executable::Standard,
        file_system::{AccessFlags, CreateFlags, Flags, Position},
        virtual_file_system::File,
    };

    let standard = testing::initialize(false, false).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;
    context
        .task_manager
        .set_environment_variable(task, "User", "administrator")
        .await
        .unwrap();
    let filesystem = &context.virtual_file_system;
    let flags = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let mut input = File::open(filesystem, task, "/input", flags).await.unwrap();
    let output = File::open(filesystem, task, "/output", flags)
        .await
        .unwrap();
    let errors = File::open(filesystem, task, "/errors", flags)
        .await
        .unwrap();
    input.write(b"false\necho survived\nexit\n").await.unwrap();
    input.flush().await.unwrap();
    input.set_position(&Position::Start(0)).await.unwrap();
    let child_standard = standard;
    let standard = Standard::new(
        context,
        File::open(filesystem, task, "/input", AccessFlags::Read.into())
            .await
            .unwrap(),
        output,
        errors,
    );
    let mut shell = command_line_shell::Shell::new(standard).await;

    xila::executable::mount_executables!(
        context,
        filesystem,
        task,
        &[(
            &"/applications/executables/command_line_shell",
            command_line_shell::ShellExecutable
        )]
    )
    .await
    .unwrap();

    assert!(shell.main(vec!["true".into()]).await.is_ok());
    assert_eq!(shell.main(vec!["false".into()]).await.unwrap_err().get(), 1);
    assert!(shell.main(vec![]).await.is_ok());

    assert!(
        shell
            .main(vec!["cd".into(), "--invalid".into()])
            .await
            .is_err()
    );
    drop(shell);
    let mut output = File::open(filesystem, task, "/output", AccessFlags::Read.into())
        .await
        .unwrap();
    let mut buffer = [0; 512];
    let size = output.read(&mut buffer).await.unwrap();
    assert!(
        core::str::from_utf8(&buffer[..size])
            .unwrap()
            .contains("survived")
    );
    let mut shell = command_line_shell::Shell::new(child_standard).await;
    assert_eq!(
        shell
            .main(vec![
                "/applications/executables/command_line_shell".into(),
                "false".into()
            ])
            .await
            .unwrap_err()
            .get(),
        1
    );
}
