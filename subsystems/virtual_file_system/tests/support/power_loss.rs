//! Power-cut sweep through the VFS: a fault-injecting block device stops
//! writes after every possible number of program operations during a file
//! create and a directory create, the file system is remounted, and the entry
//! must be absent, complete with the requested owner and permissions, or
//! attribute-less and removable through the VFS (as root, as the parent
//! owner, and replaceable by an exclusive create or a mkdir).

use super::*;
use alloc::vec;
use file_system::{
    ControlCommand, DirectBaseOperations, DirectBlockDevice, MountOperations, Permissions, Size,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use virtual_file_system::{File, MountOptions, VirtualFileSystem};

/// In-memory flash-like device: program operations are counted, and after
/// `fail_after` of them writes and erases fail, simulating a power loss.
struct FaultDevice {
    inner: &'static MemoryDevice<512>,
    progs: AtomicUsize,
    fail_after: AtomicUsize,
    failed: AtomicBool,
}

impl FaultDevice {
    fn new() -> &'static Self {
        Box::leak(Box::new(Self {
            inner: MemoryDevice::<512>::new_static(1024 * 512),
            progs: AtomicUsize::new(0),
            fail_after: AtomicUsize::new(usize::MAX),
            failed: AtomicBool::new(false),
        }))
    }

    fn reset_progs(&self) {
        self.progs.store(0, Ordering::SeqCst);
    }

    fn progs(&self) -> usize {
        self.progs.load(Ordering::SeqCst)
    }

    fn fail_after(&self, count: usize) {
        self.fail_after.store(count, Ordering::SeqCst);
    }

    fn allow_all(&self) {
        self.fail_after.store(usize::MAX, Ordering::SeqCst);
        self.failed.store(false, Ordering::SeqCst);
    }
}

impl DirectBaseOperations for FaultDevice {
    fn read(
        &self,
        context: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        position: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        self.inner.read(context, buffer, position)
    }

    fn write(
        &self,
        context: &mut core::task::Context<'_>,
        buffer: &[u8],
        position: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        let count = self.progs.fetch_add(1, Ordering::SeqCst) + 1;
        if self.failed.load(Ordering::SeqCst) || count > self.fail_after.load(Ordering::SeqCst) {
            self.failed.store(true, Ordering::SeqCst);
            return core::task::Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        self.inner.write(context, buffer, position)
    }

    fn flush(
        &self,
        context: &mut core::task::Context<'_>,
    ) -> core::task::Poll<file_system::Result<()>> {
        self.inner.flush(context)
    }

    fn control(
        &self,
        context: &mut core::task::Context<'_>,
        command: file_system::ControlCommandIdentifier,
        input: &shared::AnyByLayout,
        output: &mut shared::AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        if command == file_system::block_device::ERASE::IDENTIFIER {
            // After the power cut, erases fail too. An erase is not a program
            // operation: zero the block directly on the inner device so it
            // does not count towards the program count.
            if self.failed.load(Ordering::SeqCst) {
                return core::task::Poll::Ready(Err(IoErrorKind::InputOutput));
            }
            let range = file_system::block_device::ERASE::cast_input(input)?;
            let block = vec![0u8; range.size as usize];
            return match self.inner.write(context, &block, range.position) {
                core::task::Poll::Ready(Ok(_)) => core::task::Poll::Ready(Ok(())),
                core::task::Poll::Ready(Err(error)) => core::task::Poll::Ready(Err(error)),
                core::task::Poll::Pending => core::task::Poll::Pending,
            };
        }
        self.inner.control(context, command, input, output)
    }
}

impl MountOperations for FaultDevice {}
impl DirectBlockDevice for FaultDevice {}

fn options() -> little_fs::Options {
    little_fs::Options::new(256)
}

fn set_clock(time_manager: &time::Manager<'static>, seconds: i128) {
    time_manager
        .set_realtime(time::RealtimeSample {
            timestamp: time::Timestamp::from_nanos(seconds * 1_000_000_000),
            source: time::ClockSource::Manual,
            resolution: core::time::Duration::from_secs(1),
            accuracy: None,
        })
        .unwrap();
}

async fn spawn(
    device: &'static FaultDevice,
) -> (
    TaskIdentifier,
    &'static VirtualFileSystem,
    &'static task::Manager,
) {
    let time_manager = Box::leak(Box::new(time::Manager::unsynchronized()));
    set_clock(time_manager, 1_000);
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    if !log::is_initialized() {
        log::initialize(&testing_support::Logger).unwrap();
    }
    let task = task_manager.get_current_task_identifier().await;
    let file_system = little_fs::FileSystem::new(device, options()).unwrap();
    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize_with_options(
            task_manager,
            users_manager,
            time_manager,
            file_system,
            MountOptions::default(),
        )
        .unwrap(),
    ));
    (task, virtual_file_system, task_manager)
}

enum CutState {
    Absent,
    Complete,
    AttributeLess,
}

/// Classify an entry after remount: absent, complete (owner, permissions, kind
/// as the VFS created them), or attribute-less (fail closed).
async fn classify(
    virtual_file_system: &'static VirtualFileSystem,
    path: &str,
    kind: file_system::Kind,
    permissions: Permissions,
) -> CutState {
    match virtual_file_system.get_statistics(&Path::new(path)).await {
        Err(IoErrorKind::NotFound) => CutState::Absent,
        Err(IoErrorKind::MissingAttribute) => CutState::AttributeLess,
        Ok(statistics) => {
            assert_eq!(statistics.kind, kind);
            assert_eq!(statistics.user, users::UserIdentifier::ROOT);
            assert_eq!(statistics.permissions, permissions);
            CutState::Complete
        }
        Err(error) => panic!("unexpected error for {path}: {error:?}"),
    }
}

fn exclusive_create_flags() -> Flags {
    Flags::new(
        AccessFlags::READ_WRITE,
        Some(CreateFlags::CREATE_EXCLUSIVE),
        None,
    )
}

#[task::test]
async fn power_cut_during_file_create_leaves_absent_complete_or_removable_entry() {
    // Program operations a VFS file create needs on a freshly mounted device.
    let probe = FaultDevice::new();
    little_fs::FileSystem::format(probe, options()).unwrap();
    let (task, virtual_file_system, _) = spawn(probe).await;
    probe.reset_progs();
    let file = File::open(
        virtual_file_system,
        task,
        "/probe",
        exclusive_create_flags(),
    )
    .await
    .unwrap();
    file.close(virtual_file_system).await.unwrap();
    let total = probe.progs();
    assert_eq!(total, 2);

    for cut in 0..=total {
        let device = FaultDevice::new();
        little_fs::FileSystem::format(device, options()).unwrap();
        let (task, virtual_file_system, _) = spawn(device).await;
        device.reset_progs();
        device.fail_after(cut);
        if let Ok(file) =
            File::open(virtual_file_system, task, "/cut", exclusive_create_flags()).await
        {
            let _ = file.close(virtual_file_system).await;
        }
        // Simulate the power loss: the leaked VFS is never dropped, so no
        // unmount runs and no further writes happen. The power comes back
        // for the remount.
        device.allow_all();

        let (task, virtual_file_system, _) = spawn(device).await;
        match classify(
            virtual_file_system,
            "/cut",
            file_system::Kind::File,
            Permissions::FILE_DEFAULT,
        )
        .await
        {
            CutState::Absent => {}
            CutState::Complete => {
                // An exclusive create over a complete entry still fails.
                assert!(matches!(
                    File::open(virtual_file_system, task, "/cut", exclusive_create_flags()).await,
                    Err(IoErrorKind::AlreadyExists)
                ));
            }
            CutState::AttributeLess => {
                // Removable through the VFS as root.
                virtual_file_system.remove(task, "/cut").await.unwrap();
                assert!(matches!(
                    classify(
                        virtual_file_system,
                        "/cut",
                        file_system::Kind::File,
                        Permissions::FILE_DEFAULT,
                    )
                    .await,
                    CutState::Absent
                ));
                // An exclusive re-create of the same name now succeeds and is
                // complete.
                let file = File::open(virtual_file_system, task, "/cut", exclusive_create_flags())
                    .await
                    .unwrap();
                file.close(virtual_file_system).await.unwrap();
                assert!(matches!(
                    classify(
                        virtual_file_system,
                        "/cut",
                        file_system::Kind::File,
                        Permissions::FILE_DEFAULT,
                    )
                    .await,
                    CutState::Complete
                ));
            }
        }
    }
}

#[task::test]
async fn power_cut_during_directory_create_leaves_absent_complete_or_removable_entry() {
    let probe = FaultDevice::new();
    little_fs::FileSystem::format(probe, options()).unwrap();
    let (task, virtual_file_system, _) = spawn(probe).await;
    probe.reset_progs();
    virtual_file_system
        .create_directory(task, &Path::new("/probe"))
        .await
        .unwrap();
    let total = probe.progs();

    for cut in 0..=total {
        let device = FaultDevice::new();
        little_fs::FileSystem::format(device, options()).unwrap();
        let (task, virtual_file_system, _) = spawn(device).await;
        device.reset_progs();
        device.fail_after(cut);
        let _ = virtual_file_system
            .create_directory(task, &Path::new("/cut"))
            .await;
        // Simulate the power loss: the leaked VFS is never dropped, so no
        // unmount runs and no further writes happen. The power comes back
        // for the remount.
        device.allow_all();

        let (task, virtual_file_system, _) = spawn(device).await;
        match classify(
            virtual_file_system,
            "/cut",
            file_system::Kind::Directory,
            Permissions::DIRECTORY_DEFAULT,
        )
        .await
        {
            CutState::Absent => {}
            CutState::Complete => {
                assert_eq!(
                    virtual_file_system
                        .create_directory(task, &Path::new("/cut"))
                        .await,
                    Err(IoErrorKind::AlreadyExists)
                );
            }
            CutState::AttributeLess => {
                // A mkdir of the same name replaces the attribute-less entry.
                virtual_file_system
                    .create_directory(task, &Path::new("/cut"))
                    .await
                    .unwrap();
                assert!(matches!(
                    classify(
                        virtual_file_system,
                        "/cut",
                        file_system::Kind::Directory,
                        Permissions::DIRECTORY_DEFAULT,
                    )
                    .await,
                    CutState::Complete
                ));
            }
        }
    }
}

#[task::test]
async fn exclusive_create_replaces_an_attribute_less_entry() {
    // The file create sweep removes an attribute-less entry and re-creates it.
    // This test covers the other path: an exclusive create of the same name
    // must replace the attribute-less entry directly, with no removal first.
    let probe = FaultDevice::new();
    little_fs::FileSystem::format(probe, options()).unwrap();
    let (task, virtual_file_system, _) = spawn(probe).await;
    probe.reset_progs();
    let file = File::open(
        virtual_file_system,
        task,
        "/probe",
        exclusive_create_flags(),
    )
    .await
    .unwrap();
    file.close(virtual_file_system).await.unwrap();
    let total = probe.progs();
    assert_eq!(total, 2);

    for cut in 0..=total {
        let device = FaultDevice::new();
        little_fs::FileSystem::format(device, options()).unwrap();
        let (task, virtual_file_system, _) = spawn(device).await;
        device.reset_progs();
        device.fail_after(cut);
        if let Ok(file) =
            File::open(virtual_file_system, task, "/cut", exclusive_create_flags()).await
        {
            let _ = file.close(virtual_file_system).await;
        }
        device.allow_all();

        let (task, virtual_file_system, _) = spawn(device).await;
        if !matches!(
            classify(
                virtual_file_system,
                "/cut",
                file_system::Kind::File,
                Permissions::FILE_DEFAULT,
            )
            .await,
            CutState::AttributeLess
        ) {
            continue;
        }

        // The attribute-less entry cannot be opened without create through the
        // VFS, but an exclusive create of the same name replaces it.
        assert!(matches!(
            File::open(
                virtual_file_system,
                task,
                "/cut",
                Flags::new(AccessFlags::READ_WRITE, None, None),
            )
            .await,
            Err(IoErrorKind::MissingAttribute)
        ));
        let file = File::open(virtual_file_system, task, "/cut", exclusive_create_flags())
            .await
            .unwrap();
        file.close(virtual_file_system).await.unwrap();
        assert!(matches!(
            classify(
                virtual_file_system,
                "/cut",
                file_system::Kind::File,
                Permissions::FILE_DEFAULT,
            )
            .await,
            CutState::Complete
        ));
        return;
    }
    panic!("no cut produced an attribute-less entry");
}

#[task::test]
async fn attribute_less_entry_is_removable_by_root_and_by_the_parent_owner() {
    let user = users::UserIdentifier::new(1000);
    let group = users::GroupIdentifier::new(1000);

    let probe = FaultDevice::new();
    little_fs::FileSystem::format(probe, options()).unwrap();
    let (task, virtual_file_system, _task_manager) = spawn(probe).await;
    probe.reset_progs();
    let file = File::open(
        virtual_file_system,
        task,
        "/probe",
        exclusive_create_flags(),
    )
    .await
    .unwrap();
    file.close(virtual_file_system).await.unwrap();
    let total = probe.progs();

    for cut in 0..=total {
        let device = FaultDevice::new();
        little_fs::FileSystem::format(device, options()).unwrap();
        let (task, virtual_file_system, _task_manager) = spawn(device).await;
        virtual_file_system
            .users_manager()
            .add_group(group, "owners", &[])
            .await
            .unwrap();
        virtual_file_system
            .users_manager()
            .add_user(user, "owner", group, "/users/owner")
            .await
            .unwrap();
        // A parent directory owned by the non-root user.
        virtual_file_system
            .create_directory(task, &Path::new("/sub"))
            .await
            .unwrap();
        virtual_file_system
            .set_ownership(task, &Path::new("/sub"), Some(user), Some(group))
            .await
            .unwrap();

        device.reset_progs();
        device.fail_after(cut);
        if let Ok(file) = File::open(
            virtual_file_system,
            task,
            "/sub/cut",
            exclusive_create_flags(),
        )
        .await
        {
            let _ = file.close(virtual_file_system).await;
        }
        // Simulate the power loss: the leaked VFS is never dropped, so no
        // unmount runs and no further writes happen. The power comes back
        // for the remount.
        device.allow_all();

        let (task, virtual_file_system, task_manager) = spawn(device).await;
        virtual_file_system
            .users_manager()
            .add_group(group, "owners", &[])
            .await
            .unwrap();
        virtual_file_system
            .users_manager()
            .add_user(user, "owner", group, "/users/owner")
            .await
            .unwrap();
        // A normal system has a searchable root directory.
        virtual_file_system
            .set_permissions(task, &Path::new("/"), Permissions::DIRECTORY_DEFAULT)
            .await
            .unwrap();
        if !matches!(
            classify(
                virtual_file_system,
                "/sub/cut",
                file_system::Kind::File,
                Permissions::FILE_DEFAULT,
            )
            .await,
            CutState::AttributeLess
        ) {
            continue;
        }

        // The owner of the parent directory (write permission on it) can
        // remove the attribute-less entry; root can too (covered by the file
        // create sweep above).
        task_manager.set_user(task, user).await.unwrap();
        virtual_file_system.remove(task, "/sub/cut").await.unwrap();
        assert!(matches!(
            classify(
                virtual_file_system,
                "/sub/cut",
                file_system::Kind::File,
                Permissions::FILE_DEFAULT,
            )
            .await,
            CutState::Absent
        ));
        return;
    }
    panic!("no cut produced an attribute-less entry");
}
