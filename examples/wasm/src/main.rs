#![no_std]

extern crate alloc;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = r#"
export function selectedXilaDisk() {
    const selected = localStorage.getItem('xila.selectedDisk');
    return selected === 'xila_drive.img' || /^xila_\d{4}-\d{2}-\d{2}(?:-\d+)?\.img$/.test(selected)
        ? selected
        : 'xila_drive.img';
}
export function xilaDiskManagerReady(name) {
    window.xilaDiskManagerReady?.(name);
}
export function xilaDiskManagerError(message) {
    window.xilaDiskManagerError?.(message);
}
export function reloadXilaForDiskSwitch() {
    window.location.reload();
}
"#)]
#[cfg(target_arch = "wasm32")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = selectedXilaDisk)]
    fn selected_xila_disk() -> alloc::string::String;

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = xilaDiskManagerReady)]
    fn disk_manager_ready(name: &str);

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = xilaDiskManagerError)]
    fn disk_manager_error(message: &str);

    #[wasm_bindgen::prelude::wasm_bindgen(js_name = reloadXilaForDiskSwitch)]
    fn reload_xila_for_disk_switch();
}

#[cfg(target_arch = "wasm32")]
mod storage;

#[cfg(target_arch = "wasm32")]
static TASK_MANAGER: xila::task::Manager = xila::task::Manager::new();

#[cfg(target_arch = "wasm32")]
#[xila::task::run(
    task_path = xila::task,
    executor = drivers_wasm::executor::instantiate_static_executor!(),
    manager = &TASK_MANAGER
)]
async fn main(task_manager: &'static xila::task::Manager) {
    // AudioWorklet runs this module in a worker-like context without `window`.
    // The worklet only needs the exported audio factory, not the Xila runtime.
    if web_sys::window().is_none() {
        return;
    }

    drivers_wasm::memory::instantiate_global_allocator!();

    use alloc::boxed::Box;
    use alloc::string::ToString;
    use alloc::vec;
    use core::sync::atomic::{AtomicBool, Ordering};
    use drivers_wasm::devices::graphics::GraphicsDevices;
    use wasm_bindgen::{JsCast, closure::Closure};
    use web_sys::{HtmlElement, MouseEvent};
    use xila::bootsplash::Bootsplash;
    use xila::executable::{self, Standard, mount_executables};
    use xila::file_system::mbr::Mbr;
    use xila::file_system::{MountOperations, Path};
    use xila::log;
    use xila::task;
    use xila::time::{self, Duration};
    use xila::virtual_file_system::mount_static;
    use xila::{authentication, graphics, users, virtual_file_system};

    console_error_panic_hook::set_once();

    // - Initialize the system
    log::initialize(&drivers_wasm::log::Logger).unwrap();

    // Initialize the task manager
    let task = task_manager.get_current_task_identifier().await;
    let users_manager: &'static users::Manager = Box::leak(Box::new(users::Manager::new()));
    let time_manager: &'static time::Manager<'static> = Box::leak(Box::new(
        time::Manager::new(&drivers_wasm::devices::TimeDevice).unwrap(),
    ));

    // - Initialize the graphics manager
    // - - Initialize the graphics driver
    let GraphicsDevices {
        screen_device,
        mouse_device,
        keyboard_device,
        canvas: _canvas,
    } = drivers_wasm::devices::graphics::new().await.unwrap();

    let resolution = drivers_wasm::devices::graphics::get_resolution().unwrap();

    // - - Initialize the graphics manager
    let graphics_manager = graphics::initialize(
        time_manager,
        Box::leak(Box::new(screen_device)),
        Box::leak(Box::new(mouse_device)),
        graphics::InputKind::Pointer,
        graphics::get_recommended_buffer_size(&resolution),
        //graphics::get_minimal_buffer_size(&resolution),
        true,
    )
    .await;
    let graphics_manager: &'static graphics::Manager = Box::leak(Box::new(graphics_manager));
    graphics::set_ffi_manager(graphics_manager);

    graphics_manager
        .add_input_device(
            Box::leak(Box::new(keyboard_device)),
            graphics::InputKind::Keypad,
        )
        .await
        .unwrap();

    let bootsplash = Bootsplash::new(graphics_manager).await.unwrap();

    task_manager
        .spawn(task, "Graphics", None, move |_| {
            graphics_manager.r#loop(task::Manager::sleep)
        })
        .await
        .unwrap();

    // - Initialize the file system
    // Open the origin's persistent disk before synchronous MBR/LittleFS setup.
    let selected_disk = selected_xila_disk();
    let selected_disk_path = Path::from_str(&selected_disk);
    let drive: &'static drivers_wasm::devices::DriveDevice =
        match drivers_wasm::devices::DriveDevice::new_seeded(
            selected_disk_path,
            16 * 1024 * 1024,
            "./seed.img",
        )
        .await
        {
            Ok(drive) => Box::leak(Box::new(drive)),
            Err(error) => {
                let message = alloc::format!("Could not open {selected_disk}: {error}");
                disk_manager_error(&message);
                panic!("{message}");
            }
        };

    // Imported seeds and saved images must already contain a valid filesystem.
    let file_system = match storage::mount(drive) {
        Ok(file_system) => file_system,
        Err(error) => {
            // Never commit a failed seed import or changes from a failed mount.
            let _ = drive.discard_unsaved();
            let message = alloc::format!("Could not mount {selected_disk}: {error}");
            disk_manager_error(&message);
            panic!("{message}; saved data was not reformatted");
        }
    };

    // Print MBR information
    let mbr = Mbr::read_from_device(drive).unwrap();
    log::information!("MBR Information: {mbr}");

    // Initialize the virtual file system
    let virtual_file_system: &'static virtual_file_system::VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time_manager, file_system)
            .unwrap(),
    ));
    xila::abi_definitions::initialize(xila::abi_definitions::RuntimeContext {
        virtual_file_system,
        time_manager,
    });

    // - - Mount the devices

    // - - Create the default system hierarchy
    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .expect("Failed to create the default filesystem hierarchy");
    virtual_file_system::clear_temporary(virtual_file_system, task)
        .await
        .expect("Failed to clear temporary storage at boot");
    graphics::appearance::restore_appearance(graphics_manager, virtual_file_system, task).await;

    // - - Mount the devices
    virtual_file_system::clean_devices(virtual_file_system, task)
        .await
        .unwrap();
    let audio_button = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("audio-enable"))
        .and_then(|button| button.dyn_into::<HtmlElement>().ok())
        .expect("Failed to find the browser audio button in the help panel");

    let audio_button_for_callback = audio_button.clone();
    let audio_button_callback = Closure::wrap(Box::new(move |_event: MouseEvent| {
        let button = audio_button_for_callback.clone();
        wasm_bindgen_futures::spawn_local(async move {
            button.set_text_content(Some("Starting browser audio…"));
            let device =
                match drivers_wasm::devices::WebAudioDevice::start_from_user_gesture().await {
                    Ok(device) => device,
                    Err(error) => {
                        log::error!("Failed to start browser audio: {error:?}");
                        button.set_text_content(Some("Audio start failed; click to retry"));
                        return;
                    }
                };

            match virtual_file_system
                .mount_character_device(task, "/devices/audio/playback", device)
                .await
            {
                Ok(()) => {
                    button.set_text_content(Some("Audio enabled — launch Synth"));
                    button.set_attribute("disabled", "true").unwrap();
                }
                Err(error) => {
                    log::error!("Failed to mount browser audio: {error}");
                    button.set_text_content(Some("Audio mount failed"));
                }
            }
        });
    }) as Box<dyn FnMut(MouseEvent)>);
    audio_button
        .add_event_listener_with_callback("click", audio_button_callback.as_ref().unchecked_ref())
        .unwrap();
    audio_button_callback.forget();

    mount_static!(
        virtual_file_system,
        task,
        &[
            (
                &"/devices/standard_io/input",
                CharacterDevice,
                drivers_core::NullDevice
            ),
            (
                &"/devices/standard_io/output",
                CharacterDevice,
                drivers_core::NullDevice
            ),
            (
                &"/devices/standard_io/error",
                CharacterDevice,
                drivers_core::NullDevice
            ),
            (
                &"/devices/time/clock",
                CharacterDevice,
                drivers_wasm::devices::TimeDevice
            ),
            (
                &"/devices/compute/informations",
                CharacterDevice,
                drivers_wasm::devices::CpuInformationsDevice
            ),
            (
                &"/devices/random/source",
                CharacterDevice,
                drivers_shared::devices::RandomDevice
            ),
            (
                &"/devices/standard_io/null",
                CharacterDevice,
                drivers_core::NullDevice
            ),
            (
                &"/devices/network/http_client",
                CharacterDevice,
                drivers_wasm::devices::HttpClientDevice
            ),
            (
                &"/devices/network/https_client",
                CharacterDevice,
                drivers_wasm::devices::HttpClientDevice
            ),
            (
                &"/devices/compute/hasher",
                CharacterDevice,
                drivers_shared::devices::HashDevice
            ),
        ]
    )
    .await
    .unwrap();

    // Mount static executables

    let executable_context: &'static executable::ExecutableContext =
        Box::leak(Box::new(executable::ExecutableContext {
            task_manager,
            users_manager,
            virtual_file_system,
            graphics_manager: Some(graphics_manager),
            network_manager: None,
            time_manager,
        }));

    mount_executables!(
        executable_context,
        virtual_file_system,
        task,
        &[
            (&"/applications/executables/wasm", wasm::WasmExecutable),
            (
                &"/applications/executables/graphical_shell",
                graphical_shell::ShellExecutable
            ),
            (
                &"/applications/executables/command_line_shell",
                command_line_shell::ShellExecutable
            ),
            (
                &"/applications/executables/settings",
                settings::SettingsExecutable
            ),
            (
                &"/applications/executables/file_manager",
                file_manager::FileManagerExecutable
            ),
            (
                &"/applications/executables/terminal",
                terminal::TerminalExecutable
            ),
        ]
    )
    .await
    .unwrap();

    // - Execute the shell
    // - - Open the standard input, output and error
    let standard = Standard::open(
        &"/devices/standard_io/input",
        &"/devices/standard_io/output",
        &"/devices/standard_io/error",
        task,
        virtual_file_system,
        executable_context,
    )
    .await
    .unwrap();

    // - - Load saved accounts, or create defaults only on a fresh disk.
    let authentication_context = authentication::Context {
        virtual_file_system,
        task_manager,
        users_manager,
        task,
    };
    storage::initialize_users(&authentication_context, drive.is_new())
        .await
        .expect("Failed to initialize users and groups");

    drive
        .sync()
        .await
        .expect("Failed to save OPFS disk at boot");
    log::information!("OPFS disk saved at boot");

    // Main-thread OPFS commits are asynchronous. Keep one autosave running and
    // explicitly join it before the final shutdown save.
    let autosave_running: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(true)));
    virtual_file_system
        .mount_character_device(
            task,
            "/devices/power/control",
            drivers_core::PowerControlDevice::new(&xila::lifecycle::SYSTEM),
        )
        .await
        .expect("Failed to mount power control");
    virtual_file_system
        .set_permissions(
            task,
            &"/devices/power/control",
            xila::file_system::Permissions::USER_READ_WRITE,
        )
        .await
        .expect("Failed to restrict power control to root");

    let (autosave, _) = task_manager
        .spawn(task, "OPFS autosave", None, move |_| async move {
            loop {
                task::Manager::sleep(Duration::from_secs(1)).await;
                if !autosave_running.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(error) = drive.sync().await {
                    log::error!("Failed to save OPFS disk: {error}");
                }
            }
        })
        .await
        .unwrap();

    let switch_requested: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
    let runtime_running: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(true)));
    let disk_name_for_switch = selected_disk.clone();
    let switch_callback: Closure<dyn FnMut()> = Closure::wrap(Box::new(move || {
        switch_requested.store(true, Ordering::Release);
    }) as Box<dyn FnMut()>);
    web_sys::window()
        .unwrap()
        .add_event_listener_with_callback(
            "xila-disk-switch",
            switch_callback.as_ref().unchecked_ref(),
        )
        .unwrap();
    switch_callback.forget();

    let (lifecycle, _) = task_manager
        .spawn(task, "Disk manager", None, move |_| async move {
            loop {
                if switch_requested.swap(false, Ordering::AcqRel) {
                    // Quiesce the block device before the first commit. If it
                    // fails, leave autosave running and let the user retry.
                    drive.stop_writes();
                    if let Err(error) = drive.sync().await {
                        drive.resume_writes();
                        let message =
                            alloc::format!("Could not save {disk_name_for_switch}: {error}");
                        disk_manager_error(&message);
                        continue;
                    }

                    autosave_running.store(false, Ordering::Release);
                    autosave.join().await;
                    drive
                        .sync()
                        .await
                        .expect("Failed to commit disk before switching");
                    drive
                        .unmount()
                        .expect("Failed to release disk before switching");
                    reload_xila_for_disk_switch();
                    return;
                }

                if !runtime_running.load(Ordering::Acquire) {
                    break;
                }

                task::Manager::sleep(Duration::from_millis(100)).await;
            }

            autosave_running.store(false, Ordering::Release);
            autosave.join().await;
        })
        .await
        .unwrap();
    disk_manager_ready(&selected_disk);

    // - - Set the environment variables
    task_manager
        .set_environment_variable(task, "Paths", "/")
        .await
        .unwrap();

    task_manager
        .set_environment_variable(task, "Host", "xila")
        .await
        .unwrap();

    // wait some time to show the bootsplash
    task::Manager::sleep(Duration::from_secs(2)).await;

    bootsplash.stop(graphics_manager).await.unwrap();

    let has_touch_screen = drivers_wasm::devices::graphics::has_touch_screen().unwrap_or(false);

    let arguments = if has_touch_screen {
        log::information!("Touch screen detected.");
        vec!["--show-keyboard".to_string()]
    } else {
        vec![]
    };

    // - - Execute the shell
    let _ = executable::execute(
        executable_context,
        "/applications/executables/graphical_shell",
        arguments,
        standard,
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    if xila::lifecycle::SYSTEM.is_shutdown_requested() {
        let _ = xila::lifecycle::SYSTEM.begin_shutdown();
    }
    runtime_running.store(false, Ordering::Release);
    lifecycle.join().await;
    virtual_file_system.uninitialize().await;
    drive.stop_writes();
    if xila::lifecycle::SYSTEM.is_shutdown_requested() {
        let _ = xila::lifecycle::SYSTEM.begin_finalize();
    }
    drive
        .sync()
        .await
        .expect("Failed to save OPFS disk at shutdown");
    drive.unmount().expect("Failed to release OPFS disk");
    if xila::lifecycle::SYSTEM.is_shutdown_requested() {
        let _ = xila::lifecycle::SYSTEM.complete();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    extern crate std;
    panic!("This executable is only for the WebAssembly target.");
}
