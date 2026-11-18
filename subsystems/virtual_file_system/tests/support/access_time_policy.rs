use super::*;
use file_system::{DirectBaseOperations, DirectBlockDevice, MountOperations, Permissions, Size};
use std::sync::atomic::{AtomicUsize, Ordering};
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{AccessTimePolicy, Directory, Error, MountOptions};

#[allow(
    clippy::disallowed_types,
    reason = "Host-only test device uses std atomics."
)]
struct CountingDevice {
    inner: &'static MemoryDevice<512>,
    writes: AtomicUsize,
}

impl CountingDevice {
    #[allow(
        clippy::disallowed_types,
        reason = "Host-only test device uses std atomics."
    )]
    fn new() -> &'static Self {
        Box::leak(Box::new(Self {
            inner: MemoryDevice::<512>::new_static(1024 * 512),
            writes: AtomicUsize::new(0),
        }))
    }

    fn reset_write_count(&self) {
        self.writes.store(0, Ordering::SeqCst);
    }

    fn write_count(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }
}

impl DirectBaseOperations for CountingDevice {
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
        self.writes.fetch_add(1, Ordering::SeqCst);
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
        self.inner.control(context, command, input, output)
    }
}

impl MountOperations for CountingDevice {}
impl DirectBlockDevice for CountingDevice {}

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

async fn initialize_counting(
    access_time_policy: AccessTimePolicy,
    initial_seconds: i128,
) -> (
    TaskIdentifier,
    &'static VirtualFileSystem,
    &'static time::Manager<'static>,
    &'static CountingDevice,
) {
    let time_manager = Box::leak(Box::new(time::Manager::unsynchronized()));
    set_clock(time_manager, initial_seconds);

    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    if !log::is_initialized() {
        log::initialize(&testing::Logger).unwrap();
    }
    let task = task_manager.get_current_task_identifier().await;

    let device = CountingDevice::new();
    little_fs::FileSystem::format(device, little_fs::Options::new(256)).unwrap();
    let file_system = little_fs::FileSystem::new(device, little_fs::Options::new(256)).unwrap();
    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize_with_options(
            task_manager,
            users_manager,
            time_manager,
            file_system,
            MountOptions::new().with_access_time_policy(access_time_policy),
        )
        .unwrap(),
    ));

    (task, virtual_file_system, time_manager, device)
}

async fn read_file(
    virtual_file_system: &'static VirtualFileSystem,
    task: TaskIdentifier,
    path: &str,
) -> Vec<u8> {
    let mut file = File::open(virtual_file_system, task, path, AccessFlags::Read.into())
        .await
        .unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer, 64).await.unwrap();
    file.close(virtual_file_system).await.unwrap();
    buffer
}

async fn list_directory(
    virtual_file_system: &'static VirtualFileSystem,
    task: TaskIdentifier,
    path: &str,
) -> usize {
    let mut directory = Directory::open(virtual_file_system, task, path)
        .await
        .unwrap();
    let mut count = 0;
    while directory.read().await.unwrap().is_some() {
        count += 1;
    }
    directory.close(virtual_file_system).await.unwrap();
    count
}

#[task::test]
async fn access_time_policy_never_skips_read_writes_but_keeps_write_open_updates() {
    let (task, virtual_file_system, time_manager, device) =
        initialize_counting(AccessTimePolicy::Never, 1_000).await;

    virtual_file_system
        .create_directory(task, &"/directory")
        .await
        .unwrap();
    File::write_to_path(virtual_file_system, task, "/directory/file", b"hello")
        .await
        .unwrap();

    set_clock(time_manager, 2_000);
    device.reset_write_count();

    assert_eq!(
        read_file(virtual_file_system, task, "/directory/file").await,
        b"hello"
    );
    assert!(list_directory(virtual_file_system, task, "/directory").await >= 2);
    assert_eq!(device.write_count(), 0);

    let statistics = virtual_file_system
        .get_statistics(&"/directory/file")
        .await
        .unwrap();
    assert_eq!(statistics.access.as_u64(), 1_000);
    assert_eq!(statistics.modification.as_u64(), 1_000);

    let mut file = File::open(
        virtual_file_system,
        task,
        "/directory/file",
        AccessFlags::Write.into(),
    )
    .await
    .unwrap();
    file.write(b"world").await.unwrap();
    file.close(virtual_file_system).await.unwrap();
    assert!(device.write_count() > 0);

    let statistics = virtual_file_system
        .get_statistics(&"/directory/file")
        .await
        .unwrap();
    assert_eq!(statistics.access.as_u64(), 2_000);
    assert_eq!(statistics.modification.as_u64(), 2_000);
}

#[task::test]
async fn access_time_policy_always_writes_on_file_read_and_directory_open() {
    let (task, virtual_file_system, time_manager, device) =
        initialize_counting(AccessTimePolicy::Always, 1_000).await;

    virtual_file_system
        .create_directory(task, &"/directory")
        .await
        .unwrap();
    File::write_to_path(virtual_file_system, task, "/directory/file", b"hello")
        .await
        .unwrap();

    set_clock(time_manager, 2_000);
    device.reset_write_count();

    assert_eq!(
        read_file(virtual_file_system, task, "/directory/file").await,
        b"hello"
    );
    assert!(device.write_count() > 0);

    let statistics = virtual_file_system
        .get_statistics(&"/directory/file")
        .await
        .unwrap();
    assert_eq!(statistics.access.as_u64(), 2_000);
    assert_eq!(statistics.modification.as_u64(), 1_000);

    set_clock(time_manager, 3_000);
    device.reset_write_count();

    assert!(list_directory(virtual_file_system, task, "/directory").await >= 2);
    assert!(device.write_count() > 0);

    let directory_statistics = virtual_file_system
        .get_statistics(&"/directory")
        .await
        .unwrap();
    assert_eq!(directory_statistics.access.as_u64(), 3_000);
}

#[task::test]
async fn access_time_policy_relative_skips_recent_reads_and_updates_stale_access_time() {
    assert_eq!(
        MountOptions::default().access_time_policy,
        AccessTimePolicy::Relative
    );

    let (task, virtual_file_system, time_manager, device) =
        initialize_counting(AccessTimePolicy::Relative, 1_000).await;

    virtual_file_system
        .create_directory(task, &"/directory")
        .await
        .unwrap();
    File::write_to_path(virtual_file_system, task, "/directory/file", b"hello")
        .await
        .unwrap();

    // Recent file and directory: zero writes on read and directory listing.
    set_clock(time_manager, 2_000);
    device.reset_write_count();
    assert_eq!(
        read_file(virtual_file_system, task, "/directory/file").await,
        b"hello"
    );
    assert!(list_directory(virtual_file_system, task, "/directory").await >= 2);
    assert_eq!(device.write_count(), 0);

    // Make access time older than modification time.
    virtual_file_system
        .set_times_beneath(
            task,
            &Path::new("directory/file"),
            false,
            Path::ROOT,
            Some(file_system::Time::new(1_500)),
            Some(file_system::Time::new(1_800)),
        )
        .await
        .unwrap();

    device.reset_write_count();
    assert_eq!(
        read_file(virtual_file_system, task, "/directory/file").await,
        b"hello"
    );
    assert!(device.write_count() > 0);
    assert_eq!(
        virtual_file_system
            .get_statistics(&"/directory/file")
            .await
            .unwrap()
            .access
            .as_u64(),
        2_000
    );

    // Subsequent read at a nearby timestamp produces zero writes.
    set_clock(time_manager, 2_500);
    device.reset_write_count();
    assert_eq!(
        read_file(virtual_file_system, task, "/directory/file").await,
        b"hello"
    );
    assert_eq!(device.write_count(), 0);

    // Access time older than 24 hours triggers an update on file read,
    // while directory opens still never write access time under Relative.
    let one_day_later = 2_000 + 24 * 60 * 60;
    set_clock(time_manager, one_day_later);
    device.reset_write_count();
    assert!(list_directory(virtual_file_system, task, "/directory").await >= 2);
    assert_eq!(device.write_count(), 0);

    assert_eq!(
        read_file(virtual_file_system, task, "/directory/file").await,
        b"hello"
    );
    assert!(device.write_count() > 0);
    assert_eq!(
        virtual_file_system
            .get_statistics(&"/directory/file")
            .await
            .unwrap()
            .access
            .as_u64(),
        one_day_later as u64
    );
}

#[task::test]
async fn mounted_file_system_uses_its_own_access_time_policy() {
    let (task, virtual_file_system, time_manager, root_device) =
        initialize_counting(AccessTimePolicy::Never, 1_000).await;

    virtual_file_system
        .create_directory(task, &"/strict")
        .await
        .unwrap();

    let child_device = CountingDevice::new();
    little_fs::FileSystem::format(child_device, little_fs::Options::new(256)).unwrap();
    let child_file_system =
        little_fs::FileSystem::new(child_device, little_fs::Options::new(256)).unwrap();
    virtual_file_system
        .mount_file_system_with_options(
            child_file_system,
            "/strict",
            task,
            MountOptions::new().with_access_time_policy(AccessTimePolicy::Always),
        )
        .await
        .unwrap();

    File::write_to_path(virtual_file_system, task, "/root_file", b"root")
        .await
        .unwrap();
    File::write_to_path(virtual_file_system, task, "/strict/child_file", b"child")
        .await
        .unwrap();

    set_clock(time_manager, 2_000);
    root_device.reset_write_count();
    child_device.reset_write_count();

    assert_eq!(
        read_file(virtual_file_system, task, "/root_file").await,
        b"root"
    );
    assert_eq!(root_device.write_count(), 0);

    assert_eq!(
        read_file(virtual_file_system, task, "/strict/child_file").await,
        b"child"
    );
    assert!(child_device.write_count() > 0);
    assert_eq!(root_device.write_count(), 0);
}

#[task::test]
async fn identical_metadata_writes_are_skipped_without_changing_errors() {
    let (task, virtual_file_system, _, device) =
        initialize_counting(AccessTimePolicy::Never, 1_000).await;

    let user = UserIdentifier::new(1002);
    let group = GroupIdentifier::new(1002);
    virtual_file_system
        .users_manager()
        .add_group(group, "metadata_group", &[])
        .await
        .unwrap();
    virtual_file_system
        .users_manager()
        .add_user(user, "metadata_user", group)
        .await
        .unwrap();

    File::write_to_path(virtual_file_system, task, "/file", b"data")
        .await
        .unwrap();

    // Permissions: first change writes, identical second call writes nothing.
    device.reset_write_count();
    virtual_file_system
        .set_permissions(task, "/file", Permissions::USER_READ_WRITE)
        .await
        .unwrap();
    assert!(device.write_count() > 0);

    device.reset_write_count();
    virtual_file_system
        .set_permissions(task, "/file", Permissions::USER_READ_WRITE)
        .await
        .unwrap();
    assert_eq!(device.write_count(), 0);

    // Ownership: first change writes, identical second call writes nothing.
    device.reset_write_count();
    virtual_file_system
        .set_ownership(task, "/file", Some(user), Some(group))
        .await
        .unwrap();
    assert!(device.write_count() > 0);

    device.reset_write_count();
    virtual_file_system
        .set_ownership(task, "/file", Some(user), Some(group))
        .await
        .unwrap();
    virtual_file_system
        .set_ownership(task, "/file", Some(user), None)
        .await
        .unwrap();
    assert_eq!(device.write_count(), 0);

    // Handle metadata: identical writes are skipped while changes still persist.
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .unwrap();

    device.reset_write_count();
    file.set_permissions(Permissions::USER_READ_WRITE)
        .await
        .unwrap();
    assert_eq!(device.write_count(), 0);

    device.reset_write_count();
    file.set_permissions(Permissions::ALL_FULL).await.unwrap();
    assert!(device.write_count() > 0);

    device.reset_write_count();
    file.set_permissions(Permissions::ALL_FULL).await.unwrap();
    assert_eq!(device.write_count(), 0);

    device.reset_write_count();
    file.as_synchronous_file_mut()
        .set_times(
            Some(file_system::Time::new(1_000)),
            Some(file_system::Time::new(1_000)),
        )
        .unwrap();
    assert_eq!(device.write_count(), 0);

    device.reset_write_count();
    file.as_synchronous_file_mut()
        .set_times(None, Some(file_system::Time::new(1_500)))
        .unwrap();
    assert!(device.write_count() > 0);

    // Permission checks and handle ownership error behaviour remain unchanged.
    assert_eq!(
        file.set_owner(Some(UserIdentifier::ROOT), None).await,
        Err(Error::UnsupportedOperation)
    );
    file.close(virtual_file_system).await.unwrap();

    let other_user = UserIdentifier::new(1003);
    virtual_file_system
        .users_manager()
        .add_user(other_user, "other_user", group)
        .await
        .unwrap();
    virtual_file_system
        .task_manager()
        .set_user(task, other_user)
        .await
        .unwrap();
    assert_eq!(
        virtual_file_system
            .set_permissions(task, "/file", Permissions::ALL_FULL)
            .await,
        Err(Error::PermissionDenied)
    );
}
