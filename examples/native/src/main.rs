#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod cli;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod storage;

static TASK_MANAGER: xila::task::Manager = xila::task::Manager::new();

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[xila::task::run(
    task_path = xila::task,
    executor = drivers_std::executor::instantiate_static_executor!(),
    manager = &TASK_MANAGER
)]
async fn main(task_manager: &'static xila::task::Manager) {
    drivers_std::memory::instantiate_global_allocator!();
    #[cfg(all(feature = "profiling", target_os = "linux"))]
    drivers_std::profiling::start_from_environment().expect("Failed to start profiling snapshots");

    extern crate alloc;

    use alloc::vec;
    use core::time::Duration;
    use xila::authentication;
    use xila::bootsplash::Bootsplash;
    use xila::executable;
    use xila::executable::Standard;
    use xila::executable::mount_executables;
    use xila::file_system::AccessFlags;
    use xila::graphics;
    use xila::little_fs;
    use xila::lifecycle;
    use xila::log;
    use xila::network::{self, ADD_DNS_SERVER, ADD_IP_ADDRESS, ADD_ROUTE};
    use xila::task;
    use xila::time;
    use xila::users;
    use xila::virtual_file_system;
    use xila::virtual_file_system::File;
    use xila::virtual_file_system::mount_static;

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let config = match cli::parse(arguments.iter().map(String::as_str)) {
        Ok(config) => config,
        Err(error) if error.is_help() || error.is_version() => {
            println!("{error}");
            // Returning from this task does not stop the native executor.
            std::process::exit(0);
        }
        Err(error) => {
            eprintln!("Invalid command-line arguments: {error}\nTry --help for usage.");
            std::process::exit(2);
        }
    };
    if config.interface == cli::InterfaceMode::Graphical
        && config.display_driver == cli::DisplayDriver::Headless
        && (config.resolution.0 as usize)
            .checked_mul(config.resolution.1 as usize)
            .is_none_or(|area| area > xila::graphics::SCREEN_SNAPSHOT_MAX_PIXELS)
    {
        eprintln!(
            "Invalid headless display resolution: area exceeds the {} pixel limit.",
            xila::graphics::SCREEN_SNAPSHOT_MAX_PIXELS
        );
        std::process::exit(2);
    }

    // - Initialize the system
    log::initialize(&drivers_std::log::Logger).unwrap();

    // Initialize the task manager

    let users_manager: &'static users::Manager = Box::leak(Box::new(users::Manager::new()));
    let time_manager: &'static time::Manager<'static> = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));

    let task = task_manager.get_current_task_identifier().await;
    // - Initialize graphics only when using the graphical interface.
    let (graphics_manager, bootsplash): (Option<&'static graphics::Manager>, Option<Bootsplash>) =
        if config.interface == cli::InterfaceMode::Graphical {
            let resolution = graphics::Point::new(config.resolution.0, config.resolution.1);
            let (graphics_manager, event_runner) = match config.display_driver {
                cli::DisplayDriver::Native => {
                    let (screen_device, pointer_device, keyboard_device, runner) =
                        drivers_native::window_screen::new(resolution)
                            .await
                            .unwrap();
                    let (screen_device, pointer_device, keyboard_device) = (
                        Box::leak(Box::new(screen_device)),
                        Box::leak(Box::new(pointer_device)),
                        Box::leak(Box::new(keyboard_device)),
                    );
                    let pointer_device: &'static dyn xila::file_system::DirectCharacterDevice =
                        pointer_device;
                    let keyboard_device: &'static dyn xila::file_system::DirectCharacterDevice =
                        keyboard_device;
                    let input_devices = [
                        (pointer_device, graphics::InputKind::Pointer),
                        (keyboard_device, graphics::InputKind::Keypad),
                    ];
                    let manager = graphics::initialize_with_input_devices(
                        time_manager,
                        screen_device,
                        &input_devices,
                        graphics::get_minimal_buffer_size(&resolution),
                        true,
                    )
                    .await;
                    (manager, Some(runner))
                }
                cli::DisplayDriver::Headless => {
                    let screen_device = Box::leak(Box::new(
                        drivers_headless::HeadlessScreenDevice::new(resolution).unwrap_or_else(
                            |error| {
                                eprintln!("Failed to create headless display: {error}");
                                std::process::exit(2);
                            },
                        ),
                    ));
                    let manager = graphics::initialize_with_input_devices(
                        time_manager,
                        screen_device,
                        &[],
                        graphics::get_minimal_buffer_size(&resolution),
                        true,
                    )
                    .await;
                    (manager, None)
                }
            };
            let graphics_manager: &'static graphics::Manager =
                Box::leak(Box::new(graphics_manager));
            graphics::set_ffi_manager(graphics_manager);

            task_manager
                .spawn(task, "Graphics", None, |_| {
                    graphics_manager.r#loop(task::Manager::sleep)
                })
                .await
                .unwrap();

            if let Some(mut runner) = event_runner {
                task_manager
                    .spawn(task, "Event Loop", None, async move |_| {
                        runner.run().await;
                    })
                    .await
                    .unwrap();
            }

            let bootsplash = Bootsplash::new(graphics_manager).await.unwrap();
            (Some(graphics_manager), Some(bootsplash))
        } else {
            (None, None)
        };

    storage::prepare_disk(
        std::path::Path::new(&config.disk_path),
        std::path::Path::new(&config.seed_image),
        config.disk_size,
    )
    .expect("Failed to import reference disk; existing disks are never replaced");
    let drive = Box::leak(Box::new(
        drivers_std::drive_file::FileDriveDevice::open_existing(std::path::Path::new(
            &config.disk_path,
        ))
        .expect("Failed to open disk"),
    ));
    let file_system = little_fs::mount_disk(drive, 256)
        .expect("Failed to mount existing disk; it was not reformatted");

    // Initialize the virtual file system
    let virtual_file_system: &'static virtual_file_system::VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time_manager, file_system)
            .unwrap(),
    ));
    xila::abi_definitions::initialize(xila::abi_definitions::RuntimeContext {
        virtual_file_system,
        time_manager,
    });

    log::information!("Virtual file system initialized.");

    // - - Mount the devices

    // - - Create the default system hierarchy
    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .expect("Failed to create the default filesystem hierarchy");
    virtual_file_system::clear_temporary(virtual_file_system, task)
        .await
        .expect("Failed to clear temporary storage at boot");

    #[cfg(unix)]
    if graphics_manager.is_some() {
        virtual_file_system
            .create_directory(task, &"/devices/graphics/debug")
            .await
            .expect("Failed to create debug device directory");
        virtual_file_system
            .mount_character_device(
                task,
                "/devices/graphics/debug/transport",
                drivers_native::window_screen::DebugTransportDevice,
            )
            .await
            .expect("Failed to mount debug transport");
    }

    #[cfg(feature = "audio")]
    let mut audio_runner = {
        // Keep the mixer pinned for the lifetime of every mounted VFS stream.
        let audio_system: &'static audio::AudioSystem =
            Box::leak(Box::new(audio::AudioSystem::new().unwrap()));
        // SAFETY: the leaked service can never move or be freed.
        let audio_system = unsafe { core::pin::Pin::new_unchecked(audio_system) };
        match drivers_native::audio::new(audio_system, 1) {
            Ok(runner) => {
                virtual_file_system
                    .mount_character_device(
                        task,
                        "/devices/audio/playback",
                        audio::AudioStreamDevice::new(audio_system),
                    )
                    .await
                    .expect("Failed to mount audio playback");
                log::information!("Native audio ready");
                Some(runner)
            }
            Err(error) => {
                log::warning!("Audio unavailable; applications can run silently: {error}");
                None
            }
        }
    };

    virtual_file_system
        .mount_character_device(
            task,
            "/devices/power/control",
            drivers_core::PowerControlDevice::new(&lifecycle::SYSTEM),
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

    log::information!("Default hierarchy created.");

    if let Some(graphics) = graphics_manager {
        graphics::appearance::restore_appearance(graphics, virtual_file_system, task).await;
    }

    let network_manager: &'static network::Manager = Box::leak(Box::new(network::initialize(
        task_manager,
        virtual_file_system,
        time_manager,
        &drivers_shared::devices::RandomDevice,
    )));
    let http_client = Box::leak(Box::new(drivers_shared::devices::HttpClientDevice::new(
        network_manager,
        task_manager,
    )));
    let https_client = Box::leak(Box::new(drivers_shared::devices::HttpsClientDevice::new(
        &drivers_shared::devices::RandomDevice,
        network_manager,
        task_manager,
    )));

    mount_static!(
        virtual_file_system,
        task,
        &[
            (
                &"/devices/standard_io/input",
                CharacterDevice,
                drivers_std::console::StandardInDevice
            ),
            (
                &"/devices/standard_io/output",
                CharacterDevice,
                drivers_std::console::StandardOutDevice
            ),
            (
                &"/devices/standard_io/error",
                CharacterDevice,
                drivers_std::console::StandardErrorDevice
            ),
            (
                &"/devices/time/clock",
                CharacterDevice,
                drivers_std::devices::TimeDevice
            ),
            (
                &"/devices/compute/informations",
                CharacterDevice,
                drivers_std::devices::CpuInformationsDevice
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
                *http_client
            ),
            (
                &"/devices/network/https_client",
                CharacterDevice,
                *https_client
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

    if config.networking {
        let (interface_device, controller_device) =
            drivers_std::tuntap::new(&config.interface_name, false, true)
                .expect("Failed to create network interface.");

        network_manager
            .mount_interface(task, "tunnel0", interface_device, controller_device, None)
            .await
            .expect("Failed to mount network interface.");

        let mut file = File::open(
            virtual_file_system,
            task,
            "/devices/network/tunnel0",
            AccessFlags::READ_WRITE.into(),
        )
        .await
        .expect("Failed to open network interface file.");

        for ip_cidr in drivers_std::tuntap::IP_ADDRESSES {
            file.control(ADD_IP_ADDRESS, ip_cidr).await.ok();
        }

        for route in drivers_std::tuntap::ROUTES {
            file.control(ADD_ROUTE, route).await.ok();
        }

        for dns_server in drivers_std::tuntap::DEFAULT_DNS_SERVERS {
            file.control(ADD_DNS_SERVER, dns_server).await.ok();
        }

        file.close(virtual_file_system).await.unwrap();
    }

    // Mount static executables

    let executable_context: &'static executable::ExecutableContext =
        Box::leak(Box::new(executable::ExecutableContext {
            task_manager,
            users_manager,
            virtual_file_system,
            graphics_manager,
            network_manager: config.networking.then_some(network_manager),
            time_manager,
        }));

    mount_executables!(
        executable_context,
        virtual_file_system,
        task,
        &[
            (
                &"/applications/executables/graphical_shell",
                graphical_shell::ShellExecutable
            ),
            (
                &"/applications/executables/file_manager",
                file_manager::FileManagerExecutable
            ),
            (
                &"/applications/executables/command_line_shell",
                command_line_shell::ShellExecutable
            ),
            (
                &"/applications/executables/terminal",
                terminal::TerminalExecutable
            ),
            (
                &"/applications/executables/settings",
                settings::SettingsExecutable
            ),
            #[cfg(unix)]
            (
                &"/applications/executables/debug",
                debug_application::DebugExecutable
            ),
            (&"/applications/executables/wasm", wasm::WasmExecutable),
        ]
    )
    .await
    .unwrap();

    let authentication_context = authentication::Context {
        virtual_file_system,
        task_manager,
        users_manager,
        task,
    };
    authentication::initialize_seeded(&authentication_context)
        .await
        .expect("Failed to initialize saved accounts");

    // - - Set the environment variables
    task_manager
        .set_environment_variable(task, "Paths", "/")
        .await
        .unwrap();

    task_manager
        .set_environment_variable(task, "Host", "xila")
        .await
        .unwrap();

    if let (Some(bootsplash), Some(graphics_manager)) = (bootsplash, graphics_manager) {
        // Wait briefly to show the bootsplash.
        task::Manager::sleep(Duration::from_secs(2)).await;
        bootsplash.stop(graphics_manager).await.unwrap();
    }

    // Headless sessions have no physical input with which to open the Debug
    // application. Start its subsystem-backed transport before the shell so
    // a host agent can inspect and control the UI immediately after boot.
    if config.interface == cli::InterfaceMode::Graphical
        && config.display_driver == cli::DisplayDriver::Headless
    {
        let debug_standard = Standard::open(
            &"/devices/standard_io/null",
            &"/devices/standard_io/null",
            &"/devices/standard_io/null",
            task,
            virtual_file_system,
            executable_context,
        )
        .await
        .expect("Failed to open standard files for the headless Debug app");
        executable::execute(
            executable_context,
            &"/applications/executables/debug",
            vec!["--background".into()],
            debug_standard,
            None,
        )
        .await
        .expect("Failed to start the headless Debug app");
    }

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

    // - - Execute the shell
    let shell_path = match config.interface {
        cli::InterfaceMode::Graphical => "/applications/executables/graphical_shell",
        cli::InterfaceMode::Terminal => "/applications/executables/command_line_shell",
    };
    let shell = executable::execute(executable_context, shell_path, vec![], standard, None)
        .await
        .unwrap();
    let mut join = core::pin::pin!(shell.join());
    loop {
        // The shell must finish before its VFS resources can be released.
        use core::{
            future::{Future, poll_fn},
            task::Poll,
        };
        if poll_fn(|context| Poll::Ready(join.as_mut().poll(context)))
            .await
            .is_ready()
        {
            break;
        }
        #[cfg(feature = "audio")]
        if let Some(runner) = &audio_runner
            && let Err(error) = runner.check()
        {
            log::warning!("Native audio stopped: {error}; restart to reopen the host output");
            audio_runner = None;
        }
        task::Manager::sleep(Duration::from_millis(100)).await;
    }

    if lifecycle::SYSTEM.is_shutdown_requested() {
        let _ = lifecycle::SYSTEM.begin_shutdown();
    }
    virtual_file_system.uninitialize().await;
    #[cfg(feature = "audio")]
    drop(audio_runner);
    if lifecycle::SYSTEM.is_shutdown_requested() {
        let _ = lifecycle::SYSTEM.begin_finalize();
        let _ = lifecycle::SYSTEM.complete();
        // The native Embassy executor does not stop when its root future returns.
        std::process::exit(0);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn main() {
    panic!("This example is only for native platforms.");
}
