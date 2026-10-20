extern crate alloc;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[ignore]
#[xila::task::test(task_path = xila::task)]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate abi_definitions;

    use settings::SettingsExecutable;
    use xila::executable;
    use xila::executable::mount_executables;

    let standard = testing::initialize(true, true).await;
    let context = standard.context;
    let task = context.task_manager.get_current_task_identifier().await;

    mount_executables!(
        context,
        &context.virtual_file_system,
        task,
        &[(&"/applications/executables/settings", SettingsExecutable),]
    )
    .await
    .unwrap();

    let result = executable::execute(
        context,
        "/applications/executables/settings",
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
