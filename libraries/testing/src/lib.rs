//! Minimal std test support for Xila, without platform drivers.
//!
//! Provides the abstractions tests need (allocator, time, randomness,
//! console, hashing, a headless screen) and `initialize` (with the
//! `environment` feature enabled), which sets up a complete in-memory
//! environment. Real drivers (windows, network interfaces, host file systems)
//! belong to platform crates and their integration tests.

extern crate alloc;
extern crate std;

// Pull in the ABI exports LittleFS and guests link against.
#[cfg(feature = "environment")]
extern crate abi_definitions;

mod devices;
mod hash;
mod logger;
mod memory_manager;
#[cfg(feature = "graphics")]
mod screen;
/// Deterministic Wi-Fi control-plane mock (`drivers_core::wifi`); it never changes the host's Wi-Fi.
#[cfg(feature = "environment")]
pub use drivers_core::wifi;

pub use devices::{ConsoleDevice, RandomDevice, TimeDevice};
pub use hash::HashDevice;
pub use logger::Logger;
pub use memory_manager::MemoryManager;
#[cfg(feature = "graphics")]
pub use screen::{HeadlessError, HeadlessScreenDevice};
pub use task::test_executor::{Executor, new_thread_executor};

#[doc(hidden)]
pub use ::memory as memory_crate;

#[cfg(feature = "environment")]
use alloc::boxed::Box;
#[cfg(feature = "environment")]
use executable::{ExecutableContext, Standard};
#[cfg(feature = "environment")]
use file_system::{AccessFlags, CreateFlags, Flags, MemoryDevice, Path};
#[cfg(feature = "environment")]
use users::GroupIdentifier;
#[cfg(feature = "environment")]
use virtual_file_system::{VirtualFileSystem, create_default_hierarchy, mount_static};

#[cfg(feature = "environment")]
static TIME: TimeDevice = TimeDevice;
#[cfg(feature = "environment")]
static RANDOM: RandomDevice = RandomDevice;

#[cfg(feature = "environment")]
/// Set up an in-memory Xila environment and return the standard streams of
/// the current task: a formatted LittleFS on a memory device, the default
/// hierarchy, test devices, and an `administrator` user and group.
///
/// With `graphics_enabled`, graphics runs on a [`HeadlessScreenDevice`]
/// (800×600, no input devices). With `network_enabled`, a network manager
/// is created without interfaces or HTTP clients: tests needing real
/// connectivity belong to platform integration tests.
pub async fn initialize(graphics_enabled: bool, network_enabled: bool) -> Standard<'static> {
    let _ = ::log::initialize(&Logger);

    let task_manager = task::test_manager();
    let users: &'static users::Manager = Box::leak(Box::new(users::Manager::new()));
    let time: &'static time::Manager<'static> =
        Box::leak(Box::new(time::Manager::new(&TIME).unwrap()));

    #[cfg(feature = "graphics")]
    let graphics_manager = if graphics_enabled {
        let resolution = graphics::Point::new(800, 600);
        let screen: &'static HeadlessScreenDevice =
            Box::leak(Box::new(HeadlessScreenDevice::new(resolution).unwrap()));
        let manager = graphics::initialize_with_input_devices(
            time,
            screen,
            &[],
            graphics::get_minimal_buffer_size(&resolution),
            true,
        )
        .await;
        let manager: &'static graphics::Manager = Box::leak(Box::new(manager));
        graphics::set_ffi_manager(manager);
        task_manager
            .spawn(
                task_manager.get_current_task_identifier().await,
                "Graphics",
                None,
                |_| manager.r#loop(task::Manager::sleep),
            )
            .await
            .unwrap();
        Some(manager)
    } else {
        None
    };
    #[cfg(not(feature = "graphics"))]
    assert!(
        !graphics_enabled,
        "enable the `graphics` feature of xila-testing"
    );

    let file_system =
        little_fs::FileSystem::new_format(MemoryDevice::<512>::new_static(1024 * 512), 256)
            .unwrap();
    let virtual_file_system: &'static VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users, time, file_system).unwrap(),
    ));
    let task = task_manager.get_current_task_identifier().await;
    create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();

    mount_static!(
        virtual_file_system,
        task,
        &[
            (&"/devices/random/source", CharacterDevice, RandomDevice),
            (
                &"/devices/standard_io/input",
                CharacterDevice,
                ConsoleDevice::Input
            ),
            (
                &"/devices/standard_io/output",
                CharacterDevice,
                ConsoleDevice::Output
            ),
            (
                &"/devices/standard_io/error",
                CharacterDevice,
                ConsoleDevice::Error
            ),
            (&"/devices/time/clock", CharacterDevice, TimeDevice),
            (
                &"/devices/standard_io/null",
                CharacterDevice,
                drivers_core::NullDevice
            ),
            (&"/devices/compute/hasher", CharacterDevice, HashDevice),
        ]
    )
    .await
    .unwrap();

    let network_manager = network_enabled.then(|| {
        &*Box::leak(Box::new(network::initialize(
            task_manager,
            virtual_file_system,
            time,
            &RANDOM,
        )))
    });

    // `executable` may have `graphics` enabled by another crate even when this
    // crate's `graphics` feature is off, so construct without naming the field.
    let mut context = ExecutableContext::new(task_manager, users, virtual_file_system, time);
    context.network_manager = network_manager;
    #[cfg(feature = "graphics")]
    {
        context.graphics_manager = graphics_manager;
    }
    let executable_context: &'static ExecutableContext = Box::leak(Box::new(context));

    let group = GroupIdentifier::new(1000);
    let authentication_context = authentication::Context {
        virtual_file_system,
        task_manager,
        users_manager: users,
        task,
    };
    authentication::create_group(&authentication_context, "administrator", Some(group))
        .await
        .unwrap();
    authentication::create_user(&authentication_context, "administrator", "", group, None)
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
        &"/devices/standard_io/input",
        &"/devices/standard_io/output",
        &"/devices/standard_io/error",
        task,
        virtual_file_system,
        executable_context,
    )
    .await
    .unwrap()
}

#[cfg(feature = "environment")]
/// Copy a host file into the virtual file system, replacing `destination`.
pub async fn load_file(
    virtual_file_system: &VirtualFileSystem,
    source: impl AsRef<std::path::Path>,
    destination: impl AsRef<Path>,
) -> Result<(), LoadError> {
    let bytes = std::fs::read(source).map_err(LoadError::Host)?;
    let task = task::test_manager().get_current_task_identifier().await;
    let mut file = virtual_file_system::File::open(
        virtual_file_system,
        task,
        &destination.as_ref(),
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
    )
    .await
    .map_err(LoadError::VirtualFileSystem)?;
    file.write(&bytes)
        .await
        .map_err(LoadError::VirtualFileSystem)?;
    file.close(virtual_file_system)
        .await
        .map_err(LoadError::VirtualFileSystem)
}

#[cfg(feature = "environment")]
#[derive(Debug)]
pub enum LoadError {
    Host(std::io::Error),
    VirtualFileSystem(virtual_file_system::Error),
}
