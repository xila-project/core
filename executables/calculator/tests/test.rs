#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[xila::task::test(task_path = xila::task)]
#[ignore = "This test is meant to be run interactively"]
async fn main() {
    drivers_std::memory::instantiate_global_allocator!();

    extern crate alloc;

    use drivers_std::loader::load_to_virtual_file_system;
    use wasm::WasmExecutable;
    use xila::executable;
    use xila::executable::{build_crate, mount_executables};

    let standard = testing::initialize(true, false).await;
    let context = standard.context;
    let virtual_file_system = &context.virtual_file_system;
    let task_instance = &context.task_manager;

    let task = task_instance.get_current_task_identifier().await;

    let binary_path = build_crate(&"calculator").unwrap();
    load_to_virtual_file_system(
        task_instance,
        virtual_file_system,
        binary_path,
        "/applications/executables/calculator.wasm",
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

    let handle = executable::execute(
        context,
        "/applications/executables/wasm",
        vec!["/applications/executables/calculator.wasm".to_string()],
        standard,
        None,
    )
    .await
    .unwrap();

    // This standalone display has no graphical shell's close button. Allow a
    // normal close request from the console, without blocking the async executor.
    let close = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let request = close.clone();
    println!("Calculator is running with Wasmi. Press Enter here to close it.");
    std::thread::spawn(move || {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        request.store(true, std::sync::atomic::Ordering::Release);
    });
    let manager = context.graphics_manager.unwrap();
    loop {
        if close.load(std::sync::atomic::Ordering::Acquire)
            && manager.get_window_count().await.unwrap() > 0
        {
            let identifier = manager.get_window_identifier(0).await.unwrap();
            manager.send_window_close_request(identifier).await.unwrap();
            break;
        }
        xila::task::sleep(std::time::Duration::from_millis(10)).await;
    }

    let result = handle.join().await;

    assert!(result == 0);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
}
