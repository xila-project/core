extern crate alloc;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[ignore]
#[xila::task::test(task_path = xila::task)]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate abi_definitions;

    use command_line_shell::ShellExecutable;
    use terminal::TerminalExecutable;
    use xila::executable;
    use xila::executable::mount_executables;

    let standard = testing::initialize(true, false).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;
    let virtual_file_system = &context.virtual_file_system;

    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[
            (
                &"/applications/executables/terminal",
                TerminalExecutable::new(virtual_file_system, task)
                    .await
                    .unwrap()
            ),
            (
                &"/applications/executables/command_line_shell",
                ShellExecutable
            ),
        ]
    )
    .await
    .unwrap();

    let result = executable::execute(
        context,
        "/applications/executables/terminal",
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
