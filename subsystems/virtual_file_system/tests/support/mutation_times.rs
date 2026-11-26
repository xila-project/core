//! Mutation-driven timestamps and flash-write policy tests: a counting block
//! device records program operations while a manual clock drives the times the
//! VFS stamps on creations, writes, truncations and resizes.

use super::*;
use alloc::vec;
use file_system::{
    ControlCommand, DirectBaseOperations, DirectBlockDevice, MountOperations, Permissions, Size,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use virtual_file_system::MountOptions;

struct CountingDevice {
    inner: &'static MemoryDevice<512>,
    writes: AtomicUsize,
}

impl CountingDevice {
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
        // An erase is not a program operation: zero the block directly on the
        // inner device so it does not count towards the write count (the
        // default pattern-write fallback would count one write per byte).
        if command == file_system::block_device::ERASE::IDENTIFIER {
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
        log::initialize(&testing_support::Logger).unwrap();
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
            MountOptions::default(),
        )
        .unwrap(),
    ));

    (task, virtual_file_system, time_manager, device)
}

async fn statistics_of(
    virtual_file_system: &'static VirtualFileSystem,
    path: &str,
) -> file_system::Statistics {
    virtual_file_system.get_statistics(&path).await.unwrap()
}

#[task::test]
async fn create_writes_owner_permissions_and_times_in_the_creation_commits() {
    let (task, virtual_file_system, _time_manager, device) = initialize_counting(1_000).await;

    device.reset_write_count();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    // The backend persists the attributes with the new entry itself: two
    // commits (entry, then struct plus attributes). Rewriting them afterwards
    // would cost a third.
    assert_eq!(device.write_count(), 2);

    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.user, users::UserIdentifier::ROOT);
    assert_eq!(statistics.permissions, Permissions::FILE_DEFAULT);
    assert_eq!(statistics.creation.as_u64(), 1_000);
    assert_eq!(statistics.modification.as_u64(), 1_000);
    assert_eq!(statistics.access.as_u64(), 1_000);
    assert_eq!(statistics.status.as_u64(), 1_000);

    file.write(b"data").await.unwrap();
    file.close(virtual_file_system).await.unwrap();
}

#[task::test]
async fn modification_time_follows_writes_not_write_opens() {
    let (task, virtual_file_system, time_manager, device) = initialize_counting(1_000).await;

    File::write_to_path(virtual_file_system, task, "/file", b"hello")
        .await
        .unwrap();
    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 1_000);

    // Opening for writing alone changes no time and costs no flash write.
    set_clock(time_manager, 2_000);
    device.reset_write_count();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        AccessFlags::Write.into(),
    )
    .await
    .unwrap();
    assert_eq!(device.write_count(), 0);
    // A zero-byte write is not a mutation.
    file.write(b"").await.unwrap();
    file.close(virtual_file_system).await.unwrap();
    assert_eq!(device.write_count(), 0);
    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 1_000);
    assert_eq!(statistics.status.as_u64(), 1_000);

    // The first write of more than zero bytes updates the modification and
    // status times, committed with the data: one flash write for write+close.
    set_clock(time_manager, 3_000);
    device.reset_write_count();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        AccessFlags::Write.into(),
    )
    .await
    .unwrap();
    file.write(b"world").await.unwrap();
    file.close(virtual_file_system).await.unwrap();
    assert_eq!(device.write_count(), 1);
    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 3_000);
    assert_eq!(statistics.status.as_u64(), 3_000);
    // The access time is untouched by writes under the default relative policy.
    assert_eq!(statistics.access.as_u64(), 1_000);
}

#[task::test]
async fn several_writes_on_one_handle_then_close_is_one_flash_write() {
    let (task, virtual_file_system, time_manager, device) = initialize_counting(1_000).await;

    File::write_to_path(virtual_file_system, task, "/file", b"hello")
        .await
        .unwrap();

    // The modification time is recorded on the first write after the open only;
    // all writes and the time update are committed together on close.
    set_clock(time_manager, 4_000);
    device.reset_write_count();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        AccessFlags::Write.into(),
    )
    .await
    .unwrap();
    for index in 0..20u8 {
        file.write(&[index]).await.unwrap();
    }
    file.close(virtual_file_system).await.unwrap();
    assert_eq!(device.write_count(), 1);

    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 4_000);
    assert_eq!(statistics.status.as_u64(), 4_000);
    // The handle cursor started at 0, so the 20 one-byte writes overwrote and
    // extended the 5 existing bytes up to 20.
    assert_eq!(statistics.size, 20);

    // After a flush, the next write records the times again.
    set_clock(time_manager, 5_000);
    device.reset_write_count();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        AccessFlags::Write.into(),
    )
    .await
    .unwrap();
    file.write(b"a").await.unwrap();
    file.flush().await.unwrap();
    file.write(b"b").await.unwrap();
    file.close(virtual_file_system).await.unwrap();
    assert_eq!(device.write_count(), 2);
    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 5_000);
}

#[task::test]
async fn truncate_on_open_updates_modification_time_in_the_truncate_commit() {
    let (task, virtual_file_system, time_manager, device) = initialize_counting(1_000).await;

    File::write_to_path(virtual_file_system, task, "/file", b"hello")
        .await
        .unwrap();

    set_clock(time_manager, 5_000);
    device.reset_write_count();
    let file = File::open(
        virtual_file_system,
        task,
        "/file",
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
    )
    .await
    .unwrap();
    // Opening an existing file with O_TRUNC mutates it; the modification time
    // is recorded on the handle and committed with the truncation.
    file.close(virtual_file_system).await.unwrap();
    assert_eq!(device.write_count(), 1);

    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 5_000);
    assert_eq!(statistics.status.as_u64(), 5_000);
    assert_eq!(statistics.size, 0);
}

#[task::test]
async fn set_size_updates_modification_time_with_the_resize() {
    let (task, virtual_file_system, time_manager, device) = initialize_counting(1_000).await;

    File::write_to_path(virtual_file_system, task, "/file", b"hello")
        .await
        .unwrap();

    set_clock(time_manager, 7_000);
    device.reset_write_count();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/file",
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .unwrap();
    file.as_synchronous_file_mut().set_size(2).unwrap();
    // The resize and the new times are committed together on close: one flash
    // write in total, not one per metadata change.
    file.close(virtual_file_system).await.unwrap();
    assert_eq!(device.write_count(), 1);

    let statistics = statistics_of(virtual_file_system, "/file").await;
    assert_eq!(statistics.modification.as_u64(), 7_000);
    assert_eq!(statistics.status.as_u64(), 7_000);
    assert_eq!(statistics.size, 2);
}
