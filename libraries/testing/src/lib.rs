#![no_std]

extern crate alloc;

extern crate abi_definitions;

use alloc::boxed::Box;
use drivers_native::window_screen;
use drivers_shared::devices::RandomDevice;
use drivers_std::{devices::TimeDevice, log::Logger};
use executable::ExecutableContext;
use executable::Standard;
use file_system::{AccessFlags, MemoryDevice};
use network::{ADD_DNS_SERVER, ADD_IP_ADDRESS, ADD_ROUTE};
use users::GroupIdentifier;
use virtual_file_system::{File, ItemStatic, create_default_hierarchy, mount_static};

pub async fn initialize(graphics_enabled: bool, network_enabled: bool) -> Standard<'static> {
    log::initialize(&Logger).unwrap();

    let task_manager = task::test_manager();
    let users: &'static users::Manager = Box::leak(Box::new(users::Manager::new()));
    let time: &'static time::Manager<'static> =
        Box::leak(Box::new(time::Manager::new(&TimeDevice).unwrap()));
    let mut network_manager: Option<&'static network::Manager> = None;
    #[cfg(feature = "graphics")]
    let mut graphics_manager_option: Option<&'static graphics::Manager> = None;

    if graphics_enabled {
        let (screen_device, pointer_device, keyboard_device, mut runner) =
            window_screen::new(graphics::Point::new(800, 600))
                .await
                .unwrap();

        let graphics_manager = graphics::initialize(
            time,
            Box::leak(Box::new(screen_device)),
            Box::leak(Box::new(pointer_device)),
            graphics::InputKind::Pointer,
            1024 * 512,
            true,
        )
        .await;

        let graphics_manager: &'static graphics::Manager = Box::leak(Box::new(graphics_manager));
        graphics_manager_option = Some(graphics_manager);
        graphics::set_ffi_manager(graphics_manager);

        graphics_manager
            .add_input_device(
                Box::leak(Box::new(keyboard_device)),
                graphics::InputKind::Keypad,
            )
            .await
            .unwrap();

        task_manager
            .spawn(
                task_manager.get_current_task_identifier().await,
                "Graphics",
                None,
                |_| graphics_manager.r#loop(task::Manager::sleep),
            )
            .await
            .unwrap();

        task_manager
            .spawn(
                task_manager.get_current_task_identifier().await,
                "Window screen runner",
                None,
                async move |_| {
                    runner.run().await;
                },
            )
            .await
            .unwrap();
    }

    let memory_device = MemoryDevice::<512>::new_static(1024 * 512);

    let file_system = little_fs::FileSystem::new_format(memory_device, 256).unwrap();

    let virtual_file_system: &'static virtual_file_system::VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users, time, file_system).unwrap(),
    ));

    let task = task_manager.get_current_task_identifier().await;

    create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();

    let _ = virtual_file_system
        .create_directory(task, &"/devices/cpu")
        .await;

    virtual_file_system
        .mount_static(
            task,
            &"/devices/random",
            ItemStatic::CharacterDevice(&RandomDevice),
        )
        .await
        .unwrap();

    if network_enabled {
        let network_instance: &'static network::Manager = Box::leak(Box::new(network::initialize(
            task_manager,
            virtual_file_system,
            time,
            &drivers_shared::devices::RandomDevice,
        )));
        network_manager = Some(network_instance);

        let (interface_device, controller_device) =
            drivers_std::tuntap::new("xila0", false, true).unwrap();

        network_instance
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
            (&"/devices/null", CharacterDevice, drivers_core::NullDevice),
            (
                &"/devices/hasher",
                CharacterDevice,
                drivers_shared::devices::HashDevice
            ),
        ]
    )
    .await
    .unwrap();

    if network_enabled {
        let network_instance = network_manager.unwrap();
        let http_client = Box::leak(Box::new(drivers_shared::devices::HttpClientDevice::new(
            network_instance,
            task_manager,
        )));
        let https_client: &'static _ =
            Box::leak(Box::new(drivers_shared::devices::HttpsClientDevice::new(
                &drivers_shared::devices::RandomDevice,
                network_instance,
                task_manager,
            )));

        mount_static!(
            virtual_file_system,
            task,
            &[
                (&"/devices/http_client", CharacterDevice, *http_client),
                (&"/devices/https_client", CharacterDevice, *https_client),
            ]
        )
        .await
        .unwrap();
    }

    let group_identifier = GroupIdentifier::new(1000);
    let executable_context: &'static ExecutableContext = Box::leak(Box::new(ExecutableContext {
        task_manager,
        users_manager: users,
        virtual_file_system,
        #[cfg(feature = "graphics")]
        graphics_manager: graphics_manager_option,
        network_manager,
        time_manager: time,
    }));

    let authentication_context = authentication::Context {
        virtual_file_system,
        task_manager,
        users_manager: users,
        task,
    };
    authentication::create_group(
        &authentication_context,
        "administrator",
        Some(group_identifier),
    )
    .await
    .unwrap();

    authentication::create_user(
        &authentication_context,
        "administrator",
        "",
        group_identifier,
        None,
    )
    .await
    .unwrap();

    task_manager
        .set_environment_variable(task, "Paths", "/")
        .await
        .unwrap();

    task_manager
        .set_environment_variable(task, "Host", "xila")
        .await
        .unwrap();

    Standard::open(
        &"/devices/standard_in",
        &"/devices/standard_out",
        &"/devices/standard_error",
        task,
        virtual_file_system,
        executable_context,
    )
    .await
    .unwrap()
}
