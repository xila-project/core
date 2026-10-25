extern crate alloc;

extern crate abi_definitions;

use task::TaskIdentifier;

use drivers_native::host_file_system::{HostFileSystem, HostFileSystemOptions};
use file_system::{
    AccessFlags, AttributeFlags, Attributes, Context, CreateFlags, FileSystemOperations, Flags,
    MemoryDevice, Path, Position, StateFlags,
};
#[cfg(target_os = "linux")]
use task::test;
use virtual_file_system::{File, VirtualFileSystem};

#[path = "support/allocation.rs"]
mod allocation;

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
        File::create_unnamed_pipe(virtual_file_system, 512, StateFlags::empty())
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
    let _ = fs::remove_dir_all(root);
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

#[cfg(target_os = "linux")]
#[test]
async fn default_hierarchy_creates_canonical_roots_and_is_idempotent() {
    let (task, virtual_file_system) = initialize().await;

    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();
    task::block_on(virtual_file_system::create_default_hierarchy(
        virtual_file_system,
        task,
    ))
    .unwrap();

    for path in [
        "/system/accounts/users",
        "/system/accounts/groups",
        "/system/executables",
        "/system/libraries",
        "/system/services",
        "/configuration/system",
        "/configuration/applications",
        "/configuration/shared",
        "/devices/standard_io",
        "/applications/executables",
        "/applications/libraries",
        "/data/system/logs",
        "/data/applications",
        "/data/shared",
        "/temporary/cache",
        "/temporary/sessions",
    ] {
        assert_eq!(
            virtual_file_system
                .get_statistics(&Path::from_str(path))
                .await
                .unwrap()
                .kind,
            file_system::Kind::Directory,
            "{path} must be a directory"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
async fn clear_temporary_handles_an_empty_directory_and_repeated_calls() {
    let (task, vfs) = initialize().await;
    vfs.create_directory(task, &Path::TEMPORARY).await.unwrap();

    // LittleFS exposes these entries: cleanup must never follow them.
    let mut directory = virtual_file_system::Directory::open(vfs, task, Path::TEMPORARY)
        .await
        .unwrap();
    assert_eq!(directory.read().await.unwrap().unwrap().name, ".");
    assert_eq!(directory.read().await.unwrap().unwrap().name, "..");
    assert!(directory.read().await.unwrap().is_none());
    directory.close(vfs).await.unwrap();

    for _ in 0..2 {
        virtual_file_system::clear_temporary(vfs, task)
            .await
            .unwrap();
        assert_eq!(
            vfs.get_statistics(&Path::TEMPORARY).await.unwrap().kind,
            file_system::Kind::Directory
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
async fn clear_temporary_removes_nested_contents_and_preserves_root() {
    let (task, virtual_file_system) = initialize().await;
    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();
    virtual_file_system
        .create_directory(task, &Path::from_str("/temporary/sessions/nested"))
        .await
        .unwrap();
    let mut file = File::open(
        virtual_file_system,
        task,
        "/temporary/sessions/nested/state",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    file.write(b"temporary").await.unwrap();
    file.close(virtual_file_system).await.unwrap();

    for path in [
        "/temporary/cache/.hidden",
        "/temporary/top",
        "/data/system/logs/keep",
    ] {
        let mut file = File::open(
            virtual_file_system,
            task,
            path,
            Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
        )
        .await
        .unwrap();
        file.write(b"keep").await.unwrap();
        file.close(virtual_file_system).await.unwrap();
    }

    virtual_file_system::clear_temporary(virtual_file_system, task)
        .await
        .unwrap();

    assert_eq!(
        virtual_file_system
            .get_statistics(&Path::TEMPORARY)
            .await
            .unwrap()
            .kind,
        file_system::Kind::Directory
    );
    assert!(
        virtual_file_system
            .get_statistics(&Path::from_str("/temporary/sessions/nested/state"))
            .await
            .is_err()
    );
    assert_eq!(
        virtual_file_system
            .get_statistics(&Path::from_str("/data/system/logs"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::Directory
    );
    let mut directory =
        virtual_file_system::Directory::open(virtual_file_system, task, Path::TEMPORARY)
            .await
            .unwrap();
    while let Some(entry) = directory.read().await.unwrap() {
        assert!(entry.name == "." || entry.name == "..");
    }
    directory.close(virtual_file_system).await.unwrap();

    let mut file = File::open(
        virtual_file_system,
        task,
        "/data/system/logs/keep",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();
    let mut contents = [0; 4];
    assert_eq!(file.read(&mut contents).await.unwrap(), 4);
    assert_eq!(&contents, b"keep");
    file.close(virtual_file_system).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn clear_temporary_removes_deep_and_branching_directories() {
    let (task, vfs) = initialize().await;
    vfs.create_directory(task, &Path::TEMPORARY).await.unwrap();
    let mut path = String::from("/temporary");
    for _ in 0..32 {
        path.push_str("/d");
        vfs.create_directory(task, &Path::from_str(&path))
            .await
            .unwrap();
        let sibling = format!("{path}/sibling");
        vfs.create_directory(task, &Path::from_str(&sibling))
            .await
            .unwrap();
    }

    virtual_file_system::clear_temporary(vfs, task)
        .await
        .unwrap();
    assert!(
        vfs.get_statistics(&Path::from_str("/temporary/d"))
            .await
            .is_err()
    );
    assert_eq!(
        vfs.get_statistics(&Path::TEMPORARY).await.unwrap().kind,
        file_system::Kind::Directory
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn hierarchy_reports_non_directory_conflicts() {
    let (task, virtual_file_system) = initialize().await;
    let _file = File::open(
        virtual_file_system,
        task,
        "/system",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();

    assert_eq!(
        virtual_file_system::create_default_hierarchy(virtual_file_system, task).await,
        Err(virtual_file_system::Error::NotADirectory)
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn global_application_storage_is_private_and_user_app_storage_is_separate() {
    let (task, vfs) = initialize().await;
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();

    let group = users::GroupIdentifier::new(1000);
    let user = users::UserIdentifier::new(1000);
    vfs.users_manager()
        .add_group(group, "test-user", &[])
        .await
        .unwrap();
    vfs.users_manager()
        .add_user(user, "test-user", group)
        .await
        .unwrap();

    let global_directory = Path::from_str("/data/applications/clock");
    vfs.create_directory(task, &global_directory).await.unwrap();
    let mut global_file = File::open(
        vfs,
        task,
        "/data/applications/clock/state",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    global_file.write(b"shared across users").await.unwrap();
    global_file.close(vfs).await.unwrap();

    let first_home = vfs
        .ensure_user_application_directories(task, user, "clock")
        .await
        .unwrap();
    let second_user = users::UserIdentifier::new(1001);
    let second_home = vfs
        .ensure_user_application_directories(task, second_user, "clock")
        .await;
    assert!(second_home.is_err(), "unknown accounts must not get homes");
    assert_eq!(first_home.as_str(), "/users/1000/applications/clock");
    let user_data = first_home.append("data").unwrap();
    assert_eq!(vfs.get_statistics(&user_data).await.unwrap().user, user);
    assert_ne!(global_directory.as_str(), user_data.as_str());

    let task_manager = vfs.task_manager();
    task_manager.set_user(task, user).await.unwrap();
    let clock = application_metadata::application_identifier("clock").unwrap();
    task_manager
        .set_application_identifier(task, Some(clock))
        .await
        .unwrap();
    assert_eq!(
        task::block_on(vfs.get_statistics_for_task(task, &global_directory))
            .unwrap()
            .kind,
        file_system::Kind::Directory
    );
    File::open(
        vfs,
        task,
        "/data/applications/clock/state",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap()
    .close(vfs)
    .await
    .unwrap();

    task_manager
        .set_application_identifier(
            task,
            application_metadata::application_identifier("terminal"),
        )
        .await
        .unwrap();
    assert_eq!(
        task::block_on(vfs.get_statistics_for_task(task, &global_directory)),
        Err(virtual_file_system::Error::PermissionDenied)
    );
    assert!(matches!(
        File::open(
            vfs,
            task,
            "/data/applications/clock/state",
            AccessFlags::Read.into()
        )
        .await,
        Err(virtual_file_system::Error::PermissionDenied)
    ));
}

#[cfg(target_os = "linux")]
#[path = "support/polling.rs"]
mod polling;

#[cfg(target_os = "linux")]
#[path = "support/device_polling.rs"]
mod device_polling;

#[cfg(target_os = "linux")]
#[test]
async fn symlink_resolution_and_entry_operations_follow_vfs_semantics() {
    use virtual_file_system::Error as VfsError;

    let (task, vfs) = initialize().await;
    vfs.create_directory(task, &Path::from_str("/links"))
        .await
        .unwrap();
    let mut original = File::open(
        vfs,
        task,
        "/links/original",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    original.write(b"hello").await.unwrap();
    original.close(vfs).await.unwrap();

    for attempt in 0..8 {
        let path = alloc::format!("/links/.xila-link-stage-{attempt}");
        let mut collision = File::open(
            vfs,
            task,
            Path::from_str(&path),
            Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
        )
        .await
        .unwrap();
        if attempt == 0 {
            collision.write(b"unrelated").await.unwrap();
        }
        collision.close(vfs).await.unwrap();
    }

    vfs.create_symlink(
        task,
        Path::from_str("/links/original"),
        Path::from_str("/links/absolute"),
    )
    .await
    .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("original"),
        Path::from_str("/links/relative"),
    )
    .await
    .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("relative"),
        Path::from_str("/links/chain"),
    )
    .await
    .unwrap();

    let mut collision = File::open(
        vfs,
        task,
        "/links/.xila-link-stage-0",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();
    let mut collision_contents = [0; 9];
    assert_eq!(
        collision.read(&mut collision_contents).await.unwrap(),
        collision_contents.len()
    );
    assert_eq!(&collision_contents, b"unrelated");
    collision.close(vfs).await.unwrap();

    vfs.create_directory(task, &Path::from_str("/links/directory"))
        .await
        .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("directory"),
        Path::from_str("/links/directory-link"),
    )
    .await
    .unwrap();
    assert_eq!(
        vfs.get_link_statistics(&Path::from_str("/links/directory-link"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::SymbolicLink
    );
    assert_eq!(
        vfs.get_statistics(&Path::from_str("/links/directory-link"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::Directory
    );
    let directory = vfs
        .open_directory(task, &Path::from_str("/links/directory-link"))
        .await
        .unwrap();
    directory.close(vfs).await.unwrap();

    vfs.create_named_pipe(&Path::from_str("/links/pipe"), 32, task)
        .await
        .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("pipe"),
        Path::from_str("/links/pipe-link"),
    )
    .await
    .unwrap();
    assert_eq!(
        vfs.get_statistics(&Path::from_str("/links/pipe-link"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::Pipe
    );

    vfs.mount_block_device(
        task,
        &Path::from_str("/links/block-device"),
        MemoryDevice::<512>::new(512),
    )
    .await
    .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("block-device"),
        Path::from_str("/links/device-link"),
    )
    .await
    .unwrap();
    assert_eq!(
        vfs.get_statistics(&Path::from_str("/links/device-link"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::BlockDevice
    );

    for path in ["/links/absolute", "/links/relative", "/links/chain"] {
        let mut file = File::open(vfs, task, path, AccessFlags::Read.into())
            .await
            .unwrap();
        let mut contents = [0; 5];
        assert_eq!(file.read(&mut contents).await.unwrap(), contents.len());
        assert_eq!(&contents, b"hello");
        file.close(vfs).await.unwrap();
    }

    let link_stats = vfs
        .get_link_statistics(&Path::from_str("/links/absolute"))
        .await
        .unwrap();
    assert_eq!(link_stats.kind, file_system::Kind::SymbolicLink);
    assert_eq!(link_stats.size, "/links/original".len() as u64);
    assert_eq!(
        vfs.get_statistics(&Path::from_str("/links/absolute"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::File
    );
    let trailing_slash_stats = vfs
        .get_statistics(&Path::from_str("/links/absolute/"))
        .await;
    assert!(matches!(trailing_slash_stats, Err(VfsError::NotADirectory)));

    // A trailing slash requires a directory, but must not make entry
    // operations follow a final symlink and mutate its target.
    vfs.create_directory(task, &Path::from_str("/links/empty-directory"))
        .await
        .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("/links/empty-directory"),
        Path::from_str("/links/trailing-directory-link"),
    )
    .await
    .unwrap();
    let mut source = File::open(
        vfs,
        task,
        "/links/source",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    source.write(b"source").await.unwrap();
    source.close(vfs).await.unwrap();
    assert!(
        vfs.remove(task, Path::from_str("/links/trailing-directory-link/"))
            .await
            .is_err()
    );
    assert!(
        vfs.rename(
            task,
            &Path::from_str("/links/trailing-directory-link/"),
            &Path::from_str("/links/destination"),
        )
        .await
        .is_err()
    );
    assert!(
        vfs.rename(
            task,
            &Path::from_str("/links/source"),
            &Path::from_str("/links/trailing-directory-link/"),
        )
        .await
        .is_err()
    );
    assert_eq!(
        vfs.get_link_statistics(&Path::from_str("/links/trailing-directory-link"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::SymbolicLink
    );
    assert_eq!(
        vfs.get_statistics(&Path::from_str("/links/empty-directory"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::Directory
    );
    assert!(
        vfs.get_statistics(&Path::from_str("/links/source"))
            .await
            .is_ok()
    );

    let mut target = [0; 64];
    let length = vfs
        .read_symlink(task, Path::from_str("/links/absolute"), &mut target)
        .await
        .unwrap();
    assert_eq!(&target[..length], b"/links/original");

    vfs.create_symlink(
        task,
        Path::from_str("/missing-target"),
        Path::from_str("/links/dangling"),
    )
    .await
    .unwrap();
    assert_eq!(
        vfs.get_link_statistics(&Path::from_str("/links/dangling"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::SymbolicLink
    );
    assert!(
        vfs.get_statistics(&Path::from_str("/links/dangling"))
            .await
            .is_err()
    );

    vfs.create_symlink(
        task,
        Path::from_str("loop-b"),
        Path::from_str("/links/loop-a"),
    )
    .await
    .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("loop-a"),
        Path::from_str("/links/loop-b"),
    )
    .await
    .unwrap();
    assert!(matches!(
        File::open(vfs, task, "/links/loop-a", AccessFlags::Read.into()).await,
        Err(VfsError::FileSystem(
            file_system::Error::InvalidSymbolicLink
        ))
    ));

    assert!(matches!(
        File::open(
            vfs,
            task,
            "/links/absolute",
            Flags::new(AccessFlags::Read, Some(CreateFlags::CREATE_EXCLUSIVE), None)
        )
        .await,
        Err(VfsError::AlreadyExists)
    ));

    vfs.rename(
        task,
        &Path::from_str("/links/relative"),
        &Path::from_str("/links/renamed"),
    )
    .await
    .unwrap();
    assert_eq!(
        vfs.get_link_statistics(&Path::from_str("/links/renamed"))
            .await
            .unwrap()
            .kind,
        file_system::Kind::SymbolicLink
    );
    vfs.remove(task, Path::from_str("/links/renamed"))
        .await
        .unwrap();
    assert_eq!(
        vfs.get_statistics(&Path::from_str("/links/original"))
            .await
            .unwrap()
            .size,
        5
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn symlink_targets_can_cross_filesystem_mounts() {
    let (task, vfs) = initialize().await;
    let device = MemoryDevice::<512>::new_static(256 * 512);
    little_fs::FileSystem::format(device, 256).unwrap();
    vfs.mount_file_system(
        little_fs::FileSystem::new(device, 256).unwrap(),
        Path::from_str("/other"),
        task,
    )
    .await
    .unwrap();
    let mut target = File::open(
        vfs,
        task,
        "/other/target",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    target.write(b"mounted").await.unwrap();
    target.close(vfs).await.unwrap();

    vfs.create_symlink(
        task,
        Path::from_str("/other/target"),
        Path::from_str("/cross-mount"),
    )
    .await
    .unwrap();
    let mut link = File::open(vfs, task, "/cross-mount", AccessFlags::Read.into())
        .await
        .unwrap();
    let mut contents = [0; 7];
    assert_eq!(link.read(&mut contents).await.unwrap(), contents.len());
    assert_eq!(&contents, b"mounted");
    link.close(vfs).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn hostfs_absolute_link_uses_vfs_root_without_escaping_host_capability() {
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "xila-vfs-symlink-hostfs-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ));
    let host_root = root.join("host");
    fs::create_dir_all(&host_root).unwrap();
    fs::create_dir_all(root.join("target-directory")).unwrap();

    let direct_filesystem =
        HostFileSystem::open(&host_root, HostFileSystemOptions::default()).unwrap();
    let (task, vfs) = initialize().await;
    vfs.create_directory(task, &Path::from_str("/target-directory"))
        .await
        .unwrap();
    vfs.mount_file_system(
        HostFileSystem::open(&host_root, HostFileSystemOptions::default()).unwrap(),
        Path::from_str("/host"),
        task,
    )
    .await
    .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("/target-directory"),
        Path::from_str("/host/dirlink"),
    )
    .await
    .unwrap();

    let directory = vfs
        .open_directory(task, &Path::from_str("/host/dirlink"))
        .await
        .unwrap();
    directory.close(vfs).await.unwrap();

    let mut direct_context = Context::new_empty();
    assert!(
        direct_filesystem
            .lookup_directory(&mut direct_context, Path::from_str("/dirlink"))
            .is_err()
    );
    vfs.unmount(Path::from_str("/host"), true).await.unwrap();
    let _ = fs::remove_dir_all(root);
}

#[cfg(target_os = "linux")]
async fn unprivileged_user(vfs: &VirtualFileSystem) -> users::UserIdentifier {
    let task = vfs.task_manager().get_current_task_identifier().await;
    vfs.set_permissions(
        task,
        "/",
        file_system::Permissions::from_octal(0o755).unwrap(),
    )
    .await
    .unwrap();
    let user = users::UserIdentifier::new(1001);
    vfs.users_manager()
        .add_user(user, "unprivileged", users::GroupIdentifier::ROOT)
        .await
        .unwrap();
    user
}

#[cfg(target_os = "linux")]
#[test]
async fn owner_permissions_do_not_fall_through_to_group_or_others() {
    use file_system::Permissions;
    let (task, vfs) = initialize().await;
    let user = unprivileged_user(vfs).await;
    File::write_to_path(vfs, task, "/owned", b"data")
        .await
        .unwrap();
    vfs.set_ownership(task, "/owned", Some(user), None)
        .await
        .unwrap();
    vfs.set_permissions(task, "/owned", Permissions::from_octal(0o466).unwrap())
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert!(matches!(
        File::open(vfs, task, "/owned", AccessFlags::Write.into()).await,
        Err(virtual_file_system::Error::PermissionDenied)
    ));
}

#[cfg(target_os = "linux")]
#[test]
async fn group_permissions_do_not_fall_through_to_others() {
    use file_system::Permissions;
    let (task, vfs) = initialize().await;
    let user = unprivileged_user(vfs).await;
    File::write_to_path(vfs, task, "/foreign", b"data")
        .await
        .unwrap();
    vfs.set_permissions(task, "/foreign", Permissions::from_octal(0o646).unwrap())
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert!(matches!(
        File::open(vfs, task, "/foreign", AccessFlags::Write.into()).await,
        Err(virtual_file_system::Error::PermissionDenied)
    ));
}

#[cfg(target_os = "linux")]
#[test]
async fn owner_can_chmod_path_but_cannot_change_ownership() {
    use file_system::Permissions;
    let (task, vfs) = initialize().await;
    let user = unprivileged_user(vfs).await;
    File::write_to_path(vfs, task, "/owned", b"data")
        .await
        .unwrap();
    vfs.set_ownership(task, "/owned", Some(user), None)
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    vfs.set_permissions(task, "/owned", Permissions::from_octal(0o644).unwrap())
        .await
        .unwrap();
    for (uid, gid) in [
        (Some(users::UserIdentifier::ROOT), None),
        (None, Some(users::GroupIdentifier::ROOT)),
    ] {
        assert_eq!(
            vfs.set_ownership(task, "/owned", uid, gid).await,
            Err(virtual_file_system::Error::PermissionDenied)
        );
    }
}

#[cfg(target_os = "linux")]
#[path = "support/path_io.rs"]
mod path_io;

#[cfg(target_os = "linux")]
#[test]
async fn zero_capacity_named_pipe_is_rejected_without_creating_an_entry() {
    let (task, vfs) = initialize().await;
    assert_eq!(
        vfs.create_named_pipe(&"/zero", 0, task).await,
        Err(virtual_file_system::Error::FileSystem(
            file_system::Error::InvalidParameter
        ))
    );
    assert!(matches!(
        vfs.get_statistics(&"/zero").await,
        Err(virtual_file_system::Error::FileSystem(
            file_system::Error::NotFound
        ))
    ));
}

#[cfg(target_os = "linux")]
async fn mount_test_filesystem(vfs: &'static VirtualFileSystem, task: TaskIdentifier, path: &str) {
    let device = MemoryDevice::<512>::new_static(512 * 128);
    little_fs::FileSystem::format(device, 256).unwrap();
    vfs.mount_file_system(
        little_fs::FileSystem::new(device, 256).unwrap(),
        Path::new(path),
        task,
    )
    .await
    .unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn unmount_preserves_the_underlying_directory_and_contents() {
    let (task, vfs) = initialize().await;
    vfs.create_directory(task, &"/mount").await.unwrap();
    File::write_to_path(vfs, task, "/mount/original", b"kept")
        .await
        .unwrap();
    mount_test_filesystem(vfs, task, "/mount").await;
    vfs.unmount("/mount", false).await.unwrap();
    let mut bytes = [0; 4];
    File::read_slice_from_path(vfs, task, "/mount/original", &mut bytes)
        .await
        .unwrap();
    assert_eq!(&bytes, b"kept");
    mount_test_filesystem(vfs, task, "/mount").await;
    vfs.unmount("/mount", false).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn active_mount_ancestors_cannot_be_renamed_or_removed() {
    let (task, vfs) = initialize().await;
    vfs.create_directory(task, &"/ancestor").await.unwrap();
    mount_test_filesystem(vfs, task, "/ancestor/mount").await;
    assert_eq!(
        vfs.rename(task, &"/ancestor", &"/moved").await,
        Err(virtual_file_system::Error::RessourceBusy)
    );
    assert_eq!(
        vfs.remove(task, "/ancestor/mount").await,
        Err(virtual_file_system::Error::RessourceBusy)
    );
    assert_eq!(
        vfs.rename(task, &"/ancestor/mount", &"/other").await,
        Err(virtual_file_system::Error::RessourceBusy)
    );
    // A shorter shared string prefix must not reserve unrelated siblings.
    vfs.create_directory(task, &"/anc").await.unwrap();
    vfs.rename(task, &"/anc", &"/other-prefix").await.unwrap();
    vfs.remove(task, "/other-prefix").await.unwrap();
    // A component prefix must not reserve unrelated names.
    vfs.create_directory(task, &"/ancestor2").await.unwrap();
    vfs.rename(task, &"/ancestor2", &"/unrelated")
        .await
        .unwrap();
    vfs.remove(task, "/unrelated").await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn unmount_refuses_to_orphan_nested_mounts_even_when_forced() {
    let (task, vfs) = initialize().await;
    mount_test_filesystem(vfs, task, "/parent").await;
    mount_test_filesystem(vfs, task, "/parent/child").await;
    for force in [false, true] {
        assert_eq!(
            vfs.unmount("/parent", force).await,
            Err(virtual_file_system::Error::RessourceBusy)
        );
    }
    vfs.unmount("/parent/child", false).await.unwrap();
    vfs.unmount("/parent", false).await.unwrap();
}
