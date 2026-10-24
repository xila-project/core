#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[cfg(feature = "host")]
#[xila::task::test(task_path = xila::task)]
#[ignore = "The guest's stdin test requires interactive input"]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate alloc;
    extern crate abi_definitions;

    use command_line_shell::ShellExecutable;
    use drivers_std::loader::load_to_virtual_file_system;
    use wasm::WasmExecutable;
    use xila::executable::{build_crate, mount_executables};
    use xila::file_system::Path;
    use xila::log;
    use xila::virtual_file_system::File;

    let standard = testing::initialize(false, false).await;
    let context = standard.context;
    let virtual_file_system = context.virtual_file_system;
    let task = context.task_manager.get_current_task_identifier().await;

    let binary_path = build_crate("xila-wasm-wasm-test").unwrap();
    load_to_virtual_file_system(
        context.task_manager,
        virtual_file_system,
        binary_path,
        "/test_wasm.wasm",
    )
    .await
    .unwrap();

    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[
            (
                "/applications/executables/command_line_shell",
                ShellExecutable
            ),
            ("/applications/executables/wasm", WasmExecutable),
        ]
    )
    .await
    .unwrap();

    log::information!("Executing Wasmi test...");

    let result = executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/test_wasm.wasm".to_string()],
        standard,
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    assert!(result == 0);

    let mut contents = Vec::new();
    File::read_from_path(
        virtual_file_system,
        task,
        &Path::new("/test.txt"),
        &mut contents,
    )
    .await
    .unwrap();
    assert_eq!(contents, b"Hello World from WASM!");
}
