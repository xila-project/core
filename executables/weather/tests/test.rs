#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[xila::task::test(task_path = xila::task)]
#[ignore = "This test is meant to be run interactively"]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate alloc;

    use alloc::vec;
    use drivers_std::loader::load_to_virtual_file_system;
    use wasm::WasmExecutable;
    use xila::executable;
    use xila::executable::{build_crate, mount_executables};

    let standard = testing::initialize(true, true).await;
    let context = standard.context;
    let virtual_file_system = &context.virtual_file_system;
    let task_instance = &context.task_manager;

    let task = task_instance.get_current_task_identifier().await;

    let binary_path = build_crate(&"weather").unwrap();
    load_to_virtual_file_system(
        task_instance,
        virtual_file_system,
        binary_path,
        "/applications/executables/weather.wasm",
    )
    .await
    .unwrap();

    mount_executables!(
        context,
        virtual_file_system,
        task,
        &[("/applications/executables/wasm", WasmExecutable)]
    )
    .await
    .unwrap();

    let result = executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/applications/executables/weather.wasm".to_string()],
        standard,
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    assert!(result == 0);
}
