extern crate alloc;

extern crate abi_definitions;

use task::TaskIdentifier;

use drivers_native::host_file_system::{HostFileSystem, HostFileSystemOptions};
use file_system::{
    AccessFlags, AttributeFlags, Attributes, CreateFlags, FileSystemOperations, Flags,
    MemoryDevice, Path, Position, StateFlags,
};
#[cfg(target_os = "linux")]
use task::test;
use virtual_file_system::{File, VirtualFileSystem};

drivers_std::instantiate_global_allocator!();

async fn initialize() -> (TaskIdentifier, &'static VirtualFileSystem) {
    let task_instance = Box::leak(Box::new(task::Manager::new()));

    let users_manager = Box::leak(Box::new(users::Manager::new()));

    let time_manager = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));

    if !log::is_initialized() {
        log::initialize(&drivers_std::log::Logger).unwrap();
    }

    let task = task_instance.get_current_task_identifier().await;

    let device = MemoryDevice::<512>::new_static(1024 * 512);

    let cache_size = 256;

    little_fs::FileSystem::format(device, cache_size).unwrap();
    let file_system = little_fs::FileSystem::new(device, cache_size).unwrap();

    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(task_instance, users_manager, time_manager, file_system)
            .unwrap(),
    ));

    (task, virtual_file_system)
}

#[cfg(target_os = "linux")]
#[test]
async fn test_file() {
    let (task, virtual_file_system) = initialize().await;

    let file_path = "/file";

    let mut file = File::open(
        virtual_file_system,
        task,
        file_path,
        Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();

    let data = b"Hello, world!";

    file.write(data).await.unwrap();

    file.set_position(&Position::Start(0)).await.unwrap();

    let mut buffer = [0; 13];

    file.read(&mut buffer).await.unwrap();

    assert_eq!(buffer, *data);

    core::mem::drop(file);

    virtual_file_system.remove(task, file_path).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn test_unnamed_pipe() {
    let (_, virtual_file_system) = initialize().await;

    let (mut pipe_read, mut pipe_write) =
        File::create_unnamed_pipe(virtual_file_system, 512, StateFlags::None)
            .await
            .unwrap();

    let data = b"Hello, world!";

    pipe_write.write(data).await.unwrap();

    let mut buffer = [0; 13];

    pipe_read.read(&mut buffer).await.unwrap();

    assert_eq!(buffer, *data);
}

#[cfg(target_os = "linux")]
#[test]
async fn test_named_pipe() {
    let (task, virtual_file_system) = initialize().await;

    let pipe_path = "/pipe";

    virtual_file_system
        .create_named_pipe(&pipe_path, 512, task)
        .await
        .unwrap();

    let mut pipe_read = File::open(
        virtual_file_system,
        task,
        pipe_path,
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();

    let mut pipe_write = File::open(
        virtual_file_system,
        task,
        pipe_path,
        AccessFlags::Write.into(),
    )
    .await
    .unwrap();

    let data = b"Hello, world!";

    pipe_write.write(data).await.unwrap();

    let mut buffer = [0; 13];
    pipe_read.read(&mut buffer).await.unwrap();

    assert_eq!(buffer, *data);

    core::mem::drop(pipe_read);
    core::mem::drop(pipe_write);

    virtual_file_system.remove(task, pipe_path).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn forced_unmount_can_remount_the_same_path_with_a_new_filesystem() {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "xila-vfs-hostfs-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    let first_root = root.join("first");
    let second_root = root.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    fs::write(first_root.join("app"), b"old").unwrap();
    fs::write(second_root.join("app"), b"new").unwrap();

    let (task, virtual_file_system) = initialize().await;
    let first_filesystem =
        HostFileSystem::open(&first_root, HostFileSystemOptions::default()).unwrap();
    let mut host_attributes = Attributes::new().set_mask(AttributeFlags::Kind);
    FileSystemOperations::get_attributes(
        &first_filesystem,
        Path::new("/app"),
        &mut host_attributes,
    )
    .unwrap();
    virtual_file_system
        .mount_file_system(first_filesystem, Path::new("/host"), task)
        .await
        .unwrap();
    let old_file = File::open(
        virtual_file_system,
        task,
        "/host/app",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();

    virtual_file_system.unmount("/host", true).await.unwrap();
    virtual_file_system
        .mount_file_system(
            HostFileSystem::open(&second_root, HostFileSystemOptions::default()).unwrap(),
            Path::new("/host"),
            task,
        )
        .await
        .unwrap();

    let mut new_file = File::open(
        virtual_file_system,
        task,
        "/host/app",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();
    let mut data = [0; 3];
    new_file.read(&mut data).await.unwrap();
    assert_eq!(&data, b"new");

    old_file.close(virtual_file_system).await.unwrap();
    new_file.close(virtual_file_system).await.unwrap();
    let _ = fs::remove_dir_all(PathBuf::from(root));
}

#[cfg(target_os = "linux")]
#[test]
async fn test_device() {
    let (task, virtual_file_system) = initialize().await;

    const DEVICE_PATH: &Path = Path::from_str("/device");

    let device = MemoryDevice::<512>::new(512);

    virtual_file_system
        .mount_block_device(task, &DEVICE_PATH, device)
        .await
        .unwrap();

    let mut device_file = File::open(
        virtual_file_system,
        task,
        DEVICE_PATH,
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .unwrap();

    let data = 0x1234567890ABCDEF_u64;

    device_file.write(&data.to_le_bytes()).await.unwrap();

    device_file.set_position(&Position::Start(0)).await.unwrap();

    let mut buffer = [0; 8];

    device_file.read(&mut buffer).await.unwrap();

    assert_eq!(buffer, data.to_le_bytes());

    core::mem::drop(device_file);

    virtual_file_system.remove(task, DEVICE_PATH).await.unwrap();
}
