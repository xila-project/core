#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

#[cfg(test)]
extern crate std;

mod device;
mod error;
mod fundamentals;
mod manager;
mod socket;

pub use device::*;
pub use error::*;
pub use fundamentals::*;
pub use manager::*;
pub use socket::*;

#[cfg(test)]
pub mod tests {
    use alloc::boxed::Box;
    use file_system::AccessFlags;
    use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
    use virtual_file_system::{File, create_default_hierarchy};

    use super::*;

    extern crate abi_definitions;

    drivers_std::memory::instantiate_global_allocator!();

    pub(crate) static TEST_TASK_MANAGER: synchronization::once_lock::OnceLock<
        &'static task::Manager,
    > = synchronization::once_lock::OnceLock::new();

    pub(crate) async fn initialize() -> &'static crate::Manager {
        static INITIALIZE_MUTEX: Mutex<CriticalSectionRawMutex, bool> = Mutex::new(false);

        let mut initialized = INITIALIZE_MUTEX.lock().await;

        if *initialized {
            return crate::test_manager();
        }

        *initialized = true;

        static RANDOM_DEVICE: drivers_shared::devices::RandomDevice =
            drivers_shared::devices::RandomDevice;
        static TIME_DEVICE: drivers_std::devices::TimeDevice = drivers_std::devices::TimeDevice;

        let task_manager = Box::leak(Box::new(task::Manager::new()));
        TEST_TASK_MANAGER.get_or_init(|| task_manager);
        let task = task_manager.get_current_task_identifier().await;

        log::initialize(&drivers_std::log::Logger).unwrap();

        let user_manager = Box::leak(Box::new(users::Manager::new()));

        let time_manager = Box::leak(Box::new(time::Manager::new(&TIME_DEVICE).unwrap()));

        let memory_device = file_system::MemoryDevice::<512>::new_static(10 * 1024 * 1024);

        let root_file_system = little_fs::FileSystem::get_or_format(memory_device, 512).unwrap();

        let virtual_file_system = Box::leak(Box::new(
            virtual_file_system::initialize(
                task_manager,
                user_manager,
                time_manager,
                root_file_system,
            )
            .unwrap(),
        ));

        create_default_hierarchy(virtual_file_system, task)
            .await
            .unwrap();

        let network_manager = crate::TEST_MANAGER.get_or_init(|| {
            crate::Manager::new(
                task_manager,
                virtual_file_system,
                time_manager,
                &RANDOM_DEVICE,
            )
        });

        let (device, controler_device) = crate::create_loopback_device();

        let spawner = drivers_std::executor::new_thread_executor(task_manager).await;

        network_manager
            .mount_interface(task, "loopback0", device, controler_device, Some(spawner))
            .await
            .unwrap();

        let mut file = File::open(
            virtual_file_system,
            task,
            "/devices/network/loopback0",
            AccessFlags::Write.into(),
        )
        .await
        .unwrap();

        file.control(ADD_IP_ADDRESS, &IpCidr::new_ipv4([127, 0, 0, 1], 8))
            .await
            .unwrap();

        file.close(virtual_file_system).await.unwrap();

        network_manager
    }

    pub(crate) fn task_manager() -> &'static task::Manager {
        crate::tests::TEST_TASK_MANAGER
            .try_get()
            .expect("Network test task manager not initialized")
    }
}
