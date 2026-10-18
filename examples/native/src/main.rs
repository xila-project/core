#[cfg(feature = "prebuilt-applications")]
mod profiling;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod cli;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
#[inline(always)]
fn application_binary(name: &str) -> Result<std::path::PathBuf, String> {
    #[cfg(feature = "prebuilt-applications")]
    if let Some(directory) = std::env::var_os("XILA_PROFILE_APP_DIR") {
        return profiling::application_path(std::path::Path::new(&directory), name);
    }
    xila::executable::build_crate(name)
}

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
    use drivers_std::loader::load_to_virtual_file_system;
    use xila::authentication;
    use xila::bootsplash::Bootsplash;
    use xila::executable;
    use xila::executable::Standard;
    use xila::executable::mount_executables;
    use xila::file_system::AccessFlags;
    use xila::file_system::XILA_DISK_SIGNATURE;
    use xila::file_system::mbr::Mbr;
    use xila::file_system::mbr::PartitionKind;
    use xila::graphics;
    use xila::little_fs;
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
            let (screen_device, pointer_device, keyboard_device, mut runner) =
                drivers_native::window_screen::new(resolution)
                    .await
                    .unwrap();
            let (screen_device, pointer_device, keyboard_device) = (
                Box::leak(Box::new(screen_device)),
                Box::leak(Box::new(pointer_device)),
                Box::leak(Box::new(keyboard_device)),
            );

            let graphics_manager = graphics::initialize(
                time_manager,
                screen_device,
                pointer_device,
                graphics::InputKind::Pointer,
                graphics::get_minimal_buffer_size(&resolution),
                true,
            )
            .await;
            let graphics_manager: &'static graphics::Manager =
                Box::leak(Box::new(graphics_manager));
            graphics::set_ffi_manager(graphics_manager);

            graphics_manager
                .add_input_device(keyboard_device, graphics::InputKind::Keypad)
                .await
                .unwrap();

            task_manager
                .spawn(task, "Graphics", None, |_| {
                    graphics_manager.r#loop(task::Manager::sleep)
                })
                .await
                .unwrap();

            task_manager
                .spawn(task, "Event Loop", None, async move |_| {
                    runner.run().await;
                })
                .await
                .unwrap();

            let bootsplash = Bootsplash::new(graphics_manager).await.unwrap();
            (Some(graphics_manager), Some(bootsplash))
        } else {
            (None, None)
        };

    // - Initialize the file system
    // Create a memory device
    let drive = drivers_std::drive_file::FileDriveDevice::new_static(
        &config.disk_path.as_str(),
        config.disk_size,
    );
    //let drive = file_system::MemoryDevice::<512>::new_static(16 * 1024 * 1024);

    // Create a partition type
    let partition = Mbr::find_or_create_partition_with_signature(
        drive,
        XILA_DISK_SIGNATURE,
        PartitionKind::Xila,
    )
    .unwrap();

    // Print MBR information
    let mbr = Mbr::read_from_device(drive).unwrap();

    log::information!("MBR information: {mbr}");
    let partition = Box::leak(Box::new(partition));

    log::information!("Partition device: {:?}", partition);

    let file_system = little_fs::FileSystem::get_or_format(partition, 256).unwrap();

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
    let _ = virtual_file_system::create_default_hierarchy(virtual_file_system, task).await;
    let _ = virtual_file_system
        .create_directory(task, &"/devices/cpu")
        .await;

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
                &"/devices/standard_in",
                CharacterDevice,
                drivers_std::console::StandardInDevice
            ),
            (
                &"/devices/standard_out",
                CharacterDevice,
                drivers_std::console::StandardOutDevice
            ),
            (
                &"/devices/standard_error",
                CharacterDevice,
                drivers_std::console::StandardErrorDevice
            ),
            (
                &"/devices/time",
                CharacterDevice,
                drivers_std::devices::TimeDevice
            ),
            (
                &"/devices/cpu/informations",
                CharacterDevice,
                drivers_std::devices::CpuInformationsDevice
            ),
            (
                &"/devices/random",
                CharacterDevice,
                drivers_shared::devices::RandomDevice
            ),
            (&"/devices/null", CharacterDevice, drivers_core::NullDevice),
            (&"/devices/http_client", CharacterDevice, *http_client),
            (&"/devices/https_client", CharacterDevice, *https_client),
            (
                &"/devices/hasher",
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
                &"/binaries/graphical_shell",
                graphical_shell::ShellExecutable
            ),
            (
                &"/binaries/file_manager",
                file_manager::FileManagerExecutable::new(virtual_file_system, task)
                    .await
                    .unwrap()
            ),
            (
                &"/binaries/command_line_shell",
                command_line_shell::ShellExecutable
            ),
            (
                &"/binaries/terminal",
                terminal::TerminalExecutable::new(virtual_file_system, task)
                    .await
                    .unwrap()
            ),
            (
                &"/binaries/settings",
                settings::SettingsExecutable::new(virtual_file_system, task)
                    .await
                    .unwrap()
            ),
            (&"/binaries/wasm", wasm::WasmExecutable),
        ]
    )
    .await
    .unwrap();

    // - Execute the shell
    // - - Open the standard input, output and error
    let standard = Standard::open(
        &"/devices/standard_in",
        &"/devices/standard_out",
        &"/devices/standard_error",
        task,
        virtual_file_system,
        executable_context,
    )
    .await
    .unwrap();

    let calculator_binary_path = application_binary("calculator").unwrap();

    load_to_virtual_file_system(
        task_manager,
        virtual_file_system,
        &calculator_binary_path,
        "/binaries/calculator",
    )
    .await
    .unwrap();

    let weather_binary_path = application_binary("weather").unwrap();

    load_to_virtual_file_system(
        task_manager,
        virtual_file_system,
        &weather_binary_path,
        "/binaries/weather",
    )
    .await
    .unwrap();

    let clock_binary_path = application_binary("clock").unwrap();
    load_to_virtual_file_system(
        task_manager,
        virtual_file_system,
        &clock_binary_path,
        "/binaries/clock",
    )
    .await
    .unwrap();

    let pixel_painter_binary_path = application_binary("pixel_painter").unwrap();
    load_to_virtual_file_system(
        task_manager,
        virtual_file_system,
        &pixel_painter_binary_path,
        "/binaries/pixel_painter",
    )
    .await
    .unwrap();

    let _ = executable::execute(
        executable_context,
        "/binaries/wasm",
        vec!["--install".to_string(), "/binaries/calculator".to_string()],
        standard
            .duplicate()
            .await
            .expect("Failed to duplicate standard for calculator."),
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    let _ = executable::execute(
        executable_context,
        "/binaries/wasm",
        vec!["--install".to_string(), "/binaries/weather".to_string()],
        standard.duplicate().await.unwrap(),
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    let _ = executable::execute(
        executable_context,
        "/binaries/wasm",
        vec!["--install".to_string(), "/binaries/clock".to_string()],
        standard.duplicate().await.unwrap(),
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    let _ = executable::execute(
        executable_context,
        "/binaries/wasm",
        vec![
            "--install".to_string(),
            "/binaries/pixel_painter".to_string(),
        ],
        standard,
        None,
    )
    .await
    .unwrap()
    .join()
    .await;

    // - - Create the default user
    let group_identifier = users::GroupIdentifier::new(1000);

    let authentication_context = authentication::Context {
        virtual_file_system,
        task_manager,
        users_manager,
        task,
    };
    let _ = authentication::create_group(
        &authentication_context,
        "administrator",
        Some(group_identifier),
    )
    .await
    .unwrap();

    let administrator = authentication::create_user(
        &authentication_context,
        "administrator",
        "",
        group_identifier,
        None,
    )
    .await
    .unwrap();

    // The shared configuration parent belongs to root. Provision only Clock's
    // settings directory for the default login user, without widening its parent.
    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &"/configuration/shared/clock")
            .await,
    )
    .unwrap();
    virtual_file_system
        .set_ownership(
            task,
            "/configuration/shared/clock",
            Some(administrator),
            Some(group_identifier),
        )
        .await
        .unwrap();

    virtual_file_system::ignore_already_exists_error(
        virtual_file_system
            .create_directory(task, &"/configuration/shared/pixel_painter")
            .await,
    )
    .unwrap();
    virtual_file_system
        .set_ownership(
            task,
            "/configuration/shared/pixel_painter",
            Some(administrator),
            Some(group_identifier),
        )
        .await
        .unwrap();

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

    let standard = Standard::open(
        &"/devices/standard_in",
        &"/devices/standard_out",
        &"/devices/standard_error",
        task,
        virtual_file_system,
        executable_context,
    )
    .await
    .unwrap();

    // - - Execute the shell
    let shell_path = match config.interface {
        cli::InterfaceMode::Graphical => "/binaries/graphical_shell",
        cli::InterfaceMode::Terminal => "/binaries/command_line_shell",
    };
    let _ = executable::execute(executable_context, shell_path, vec![], standard, None)
        .await
        .unwrap()
        .join()
        .await;

    virtual_file_system.uninitialize().await;
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn main() {
    panic!("This example is only for native platforms.");
}
