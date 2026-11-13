//! System shutdown pipeline against a real VFS, LittleFS and power device.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use core::{
    sync::atomic::{AtomicBool, AtomicU16, Ordering},
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use drivers_peripherals::{mount_peripheral, power::PowerHandle};
use file_system::{
    AccessFlags, ControlCommandIdentifier, CreateFlags, DirectBaseOperations, Flags, MemoryDevice,
    MountOperations, Permissions, Size,
};
use lifecycle::{Action, Controller, Error, State, Supervisor};
use peripherals::power::{Error as PowerError, PowerController};
use task::{TaskIdentifier, test};
use virtual_file_system::{File, VirtualFileSystem};

/// Memory block device whose flush can be made to fail.
struct Disk {
    memory: &'static MemoryDevice<512>,
    fail_flush: AtomicBool,
}

impl DirectBaseOperations for Disk {
    fn read(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
        position: Size,
    ) -> Poll<file_system::Result<usize>> {
        self.memory.read(cx, buffer, position)
    }

    fn write(
        &self,
        cx: &mut TaskContext<'_>,
        buffer: &[u8],
        position: Size,
    ) -> Poll<file_system::Result<usize>> {
        self.memory.write(cx, buffer, position)
    }

    fn flush(&self, cx: &mut TaskContext<'_>) -> Poll<file_system::Result<()>> {
        if self.fail_flush.load(Ordering::SeqCst) {
            return Poll::Ready(Err(file_system::Error::InputOutput));
        }
        self.memory.flush(cx)
    }

    fn control(
        &self,
        cx: &mut TaskContext<'_>,
        command: ControlCommandIdentifier,
        input: &shared::AnyByLayout,
        output: &mut shared::AnyByLayout,
    ) -> Poll<file_system::Result<()>> {
        self.memory.control(cx, command, input, output)
    }
}

impl MountOperations for Disk {}
impl file_system::DirectBlockDevice for Disk {}

/// Power-off-only controller recording `action << 8 | status`.
struct Recorder(&'static AtomicU16);

impl PowerController for Recorder {
    fn capabilities(&self) -> u8 {
        device::power::Capabilities::POWER_OFF.0
    }

    fn power_off(&mut self, status: u8) -> Result<(), PowerError> {
        self.0.store(
            (Action::PowerOff as u16) << 8 | status as u16,
            Ordering::SeqCst,
        );
        Ok(())
    }
}

struct System {
    task: TaskIdentifier,
    vfs: &'static VirtualFileSystem,
    disk: &'static Disk,
    powered: &'static AtomicU16,
    time: &'static time::Manager<'static>,
}

async fn boot(disk: Option<&'static Disk>) -> System {
    if !log::is_initialized() {
        let _ = log::initialize(&testing::Logger);
    }
    let task_manager = task::test_manager();
    let task = task_manager.get_current_task_identifier().await;
    let users = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(time::Manager::new(&testing::TimeDevice).unwrap()));
    let fresh = disk.is_none();
    let disk = disk.unwrap_or_else(|| {
        Box::leak(Box::new(Disk {
            memory: MemoryDevice::<512>::new_static(1024 * 512),
            fail_flush: AtomicBool::new(false),
        }))
    });
    let file_system = little_fs::FileSystem::get_or_format(disk, 256).unwrap();
    let vfs: &'static VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users, time, file_system).unwrap(),
    ));
    if fresh {
        vfs.create_directory(task, &"/devices").await.unwrap();
    }
    let powered: &'static AtomicU16 = Box::leak(Box::new(AtomicU16::new(0)));
    let _ = vfs.remove(task, "/devices/power").await;
    mount_peripheral(
        vfs,
        task,
        "/devices/power",
        PowerHandle::new(Recorder(powered)),
    )
    .await
    .unwrap();
    vfs.set_permissions(
        task,
        "/devices/power",
        Permissions::from_octal(0o660).unwrap(),
    )
    .await
    .unwrap();
    System {
        task,
        vfs,
        disk,
        powered,
        time,
    }
}

impl System {
    fn supervisor<'a>(&self, services: &'a [TaskIdentifier], budget: Duration) -> Supervisor<'a> {
        Supervisor {
            virtual_file_system: self.vfs,
            time_manager: self.time,
            task: self.task,
            services,
            budget,
        }
    }
}

#[test]
async fn privileged_request_runs_the_pipeline_and_data_survives() {
    let system = boot(None).await;
    let (task, vfs) = (system.task, system.vfs);
    let manager = vfs.task_manager();

    // Unprivileged callers are rejected by the power device permissions.
    let users = vfs.users_manager();
    let group = users::GroupIdentifier::new(1001);
    users.add_group(group, "users", &[]).await.unwrap();
    let user = users::UserIdentifier::new(1001);
    users.add_user(user, "user", group).await.unwrap();
    manager.set_user(task, user).await.unwrap();
    assert_eq!(
        lifecycle::request(vfs, task, Action::PowerOff, 0).await,
        Err(Error::PermissionDenied)
    );
    manager
        .set_user(task, users::UserIdentifier::ROOT)
        .await
        .unwrap();
    // Unsupported actions are refused before acceptance.
    assert_eq!(
        lifecycle::request(vfs, task, Action::Sleep, 0).await,
        Err(Error::Unsupported)
    );
    assert_eq!(lifecycle::SYSTEM.state(), State::Running);

    File::write_to_path(vfs, task, "/committed", b"before shutdown")
        .await
        .unwrap();

    // A guest-like task holds an open file until asked to stop.
    let spawner = manager.get_spawner(task).await.unwrap();
    let (guest, _) = manager
        .spawn(task, "Guest", Some(spawner), async move |guest| {
            let manager = task::test_manager();
            let mut file = File::open(
                vfs,
                guest,
                "/guest",
                Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
            )
            .await
            .unwrap();
            file.write(b"guest data").await.unwrap();
            while !lifecycle::tasks::termination_requested(manager, guest).await {
                task::Manager::sleep(Duration::from_millis(5)).await;
            }
            // Cleanup: release the VFS handle before ending.
            file.close(vfs).await.unwrap();
        })
        .await
        .unwrap();

    lifecycle::request(vfs, task, Action::PowerOff, 5)
        .await
        .unwrap();
    assert_eq!(
        lifecycle::request(vfs, task, Action::PowerOff, 0).await,
        Err(Error::Busy)
    );
    assert_eq!(
        system
            .supervisor(&[], Duration::from_secs(2))
            .run(&lifecycle::SYSTEM, &mut ())
            .await,
        Ok(())
    );
    guest.join().await;
    assert_eq!(lifecycle::SYSTEM.state(), State::Returned);
    assert_eq!(
        system.powered.load(Ordering::SeqCst),
        (Action::PowerOff as u16) << 8 | 5
    );

    // No file I/O is accepted after the freeze.
    assert!(File::write_to_path(vfs, task, "/late", b"x").await.is_err());

    // Both the supervisor's and the guest's data are on the device.
    let restarted = boot(Some(system.disk)).await;
    for (path, expected) in [
        ("/committed", &b"before shutdown"[..]),
        ("/guest", b"guest data"),
    ] {
        let mut buffer = Vec::new();
        File::read_from_path(restarted.vfs, restarted.task, path, &mut buffer)
            .await
            .unwrap();
        assert_eq!(buffer, expected);
    }
}

#[test]
async fn flush_failure_stops_before_the_power_action() {
    let system = boot(None).await;
    File::write_to_path(system.vfs, system.task, "/pending", b"data")
        .await
        .unwrap();
    // The disk starts failing while tasks are being stopped, after the
    // supervisor opened the power device.
    let disk = system.disk;
    let manager = system.vfs.task_manager();
    let spawner = manager.get_spawner(system.task).await.unwrap();
    manager
        .spawn(
            system.task,
            "Disk breaker",
            Some(spawner),
            async move |task| {
                let manager = task::test_manager();
                while !lifecycle::tasks::termination_requested(manager, task).await {
                    task::Manager::sleep(Duration::from_millis(5)).await;
                }
                disk.fail_flush.store(true, Ordering::SeqCst);
            },
        )
        .await
        .unwrap();
    let controller = Controller::new();
    controller.request(Action::PowerOff, 0).unwrap();
    assert_eq!(
        system
            .supervisor(&[], Duration::from_secs(1))
            .run(&controller, &mut ())
            .await,
        Err(Error::Storage)
    );
    assert_eq!(controller.state(), State::Failed);
    assert_eq!(system.powered.load(Ordering::SeqCst), 0);
}

#[test]
async fn unresponsive_task_times_out_without_teardown() {
    static RELEASE: AtomicBool = AtomicBool::new(false);
    let system = boot(None).await;
    let manager = system.vfs.task_manager();
    let spawner = manager.get_spawner(system.task).await.unwrap();
    let vfs = system.vfs;
    let (stubborn, _) = manager
        .spawn(system.task, "Stubborn", Some(spawner), async move |task| {
            let mut file = File::open(
                vfs,
                task,
                "/held",
                Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
            )
            .await
            .unwrap();
            while !RELEASE.load(Ordering::Acquire) {
                task::Manager::sleep(Duration::from_millis(5)).await;
            }
            // Its handle is still valid: nothing was freed underneath it.
            file.write(b"still alive").await.unwrap();
            file.close(vfs).await.unwrap();
        })
        .await
        .unwrap();

    let controller = Controller::new();
    controller.request(Action::PowerOff, 0).unwrap();
    assert_eq!(
        system
            .supervisor(&[], Duration::from_millis(100))
            .run(&controller, &mut ())
            .await,
        Err(Error::Timeout)
    );
    assert_eq!(controller.state(), State::Failed);
    assert_eq!(system.powered.load(Ordering::SeqCst), 0);
    // Storage was never frozen.
    File::write_to_path(system.vfs, system.task, "/after", b"ok")
        .await
        .unwrap();

    RELEASE.store(true, Ordering::Release);
    stubborn.join().await;
}
