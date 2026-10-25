extern crate alloc;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[ignore]
#[xila::task::test(task_path = xila::task)]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate abi_definitions;

    use graphical_shell::ShellExecutable;
    use xila::executable;
    use xila::executable::mount_executables;
    use xila::virtual_file_system::File;

    let standard = testing::initialize(true, true).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;

    let virtual_file_system = &context.virtual_file_system;

    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[(
            &"/applications/executables/graphical_shell",
            ShellExecutable
        ),]
    )
    .await
    .unwrap();

    virtual_file_system
        .create_directory(task, &"/configuration/shared/applications/shortcuts")
        .await
        .unwrap();

    // Add fake shortcuts.
    for i in 0..20 {
        use alloc::format;

        File::write_to_path(
            virtual_file_system,
            task,
            format!("/configuration/shared/applications/shortcuts/test{i}.json").as_str(),
            get_shortcut_string(i).as_bytes(),
        )
        .await
        .unwrap();
    }

    let result = executable::execute(
        context,
        "/applications/executables/graphical_shell",
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

fn get_shortcut_string(index: usize) -> alloc::string::String {
    alloc::format!(
        r#"{{
    "name": "Test{index}",
    "command": "/applications/executables/?",
    "arguments": ["test"],
    "terminal": false,
    "icon_string": "T!",
    "icon_color": [255, 0, 0]
}}"#
    )
}
