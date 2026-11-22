#![allow(
    clippy::disallowed_types,
    reason = "host-only integration test uses std::sync::{Arc, atomic}"
)]

extern crate alloc;

use task::TaskIdentifier;

use file_system::{
    AccessFlags, Context, CreateFlags, Flags, IoErrorKind, MemoryDevice, Path, Position, StateFlags,
};
#[cfg(target_os = "linux")]
use task::test;
use virtual_file_system::{File, VirtualFileSystem};

#[path = "support/allocation.rs"]
mod allocation;

#[cfg(target_os = "linux")]
#[path = "support/access_time_policy.rs"]
mod access_time_policy;

#[cfg(target_os = "linux")]
#[path = "support/metadata_authorization.rs"]
mod metadata_authorization;

async fn initialize() -> (TaskIdentifier, &'static VirtualFileSystem) {
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    initialize_with_time(time_manager).await
}

async fn initialize_with_time(
    time_manager: &'static time::Manager<'static>,
) -> (TaskIdentifier, &'static VirtualFileSystem) {
    let task_instance = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));

    if !log::is_initialized() {
        log::initialize(&testing_support::Logger).unwrap();
    }

    let task = task_instance.get_current_task_identifier().await;

    let device = MemoryDevice::<512>::new_static(1024 * 512);

    let options = little_fs::Options::new(256);

    little_fs::FileSystem::format(device, options).unwrap();
    let file_system = little_fs::FileSystem::new(device, options).unwrap();

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
    async fn mount_with_app(
        virtual_file_system: &'static VirtualFileSystem,
        task: TaskIdentifier,
        contents: &[u8],
    ) {
        let file_system = little_fs::FileSystem::new_format(
            MemoryDevice::<512>::new_static(256 * 512),
            little_fs::Options::new(256),
        )
        .unwrap();
        virtual_file_system
            .mount_file_system(file_system, Path::new("/host"), task)
            .await
            .unwrap();
        let mut file = File::open(
            virtual_file_system,
            task,
            "/host/app",
            Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
        )
        .await
        .unwrap();
        file.write(contents).await.unwrap();
        file.close(virtual_file_system).await.unwrap();
    }

    let (task, virtual_file_system) = initialize().await;
    virtual_file_system
        .create_directory(task, &Path::new("/host"))
        .await
        .ok();
    mount_with_app(virtual_file_system, task, b"old").await;
    let old_file = File::open(
        virtual_file_system,
        task,
        "/host/app",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();

    virtual_file_system.unmount("/host", true).await.unwrap();
    mount_with_app(virtual_file_system, task, b"new").await;

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

    // The stale handle's file system is gone; LittleFS reports NotMounted.
    // (Platforms' host file system integration tests cover closing it cleanly.)
    let _ = old_file.close(virtual_file_system).await;
    new_file.close(virtual_file_system).await.unwrap();
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
        "/configuration/system/packages",
        "/configuration/applications",
        "/configuration/shared",
        "/devices/standard_io",
        "/applications/executables",
        "/applications/libraries",
        "/applications/bundles",
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

    for path in ["/data/shared", "/temporary", "/temporary/cache"] {
        assert_eq!(
            virtual_file_system
                .get_statistics(&Path::from_str(path))
                .await
                .unwrap()
                .permissions,
            file_system::Permissions::SHARED_DIRECTORY,
            "{path} must have shared directory permissions"
        );
    }
    assert_eq!(
        virtual_file_system
            .get_statistics(&Path::from_str("/temporary/sessions"))
            .await
            .unwrap()
            .permissions,
        file_system::Permissions::DIRECTORY_DEFAULT
    );
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
    // Only the default hierarchy's subdirectories remain, emptied.
    for (path, kept) in [
        (Path::TEMPORARY, &["cache", "sessions"][..]),
        (Path::TEMPORARY_CACHE, &[][..]),
        (Path::TEMPORARY_SESSIONS, &[][..]),
    ] {
        let mut directory = virtual_file_system::Directory::open(virtual_file_system, task, path)
            .await
            .unwrap();
        while let Some(entry) = directory.read().await.unwrap() {
            assert!(
                entry.name == "." || entry.name == ".." || kept.contains(&entry.name.as_str()),
                "{} left in {}",
                entry.name,
                path.as_str()
            );
        }
        directory.close(virtual_file_system).await.unwrap();
    }

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
        Err(IoErrorKind::NotADirectory)
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn homes_come_from_the_account_record_not_the_current_name() {
    let (task, vfs) = initialize().await;
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();
    let users_manager = vfs.users_manager();
    let group = users::GroupIdentifier::new(1000);
    users_manager.add_group(group, "people", &[]).await.unwrap();

    // Root has no account record and a fixed home.
    assert_eq!(
        vfs.get_user_home(users::UserIdentifier::ROOT)
            .await
            .unwrap()
            .as_str(),
        users::ROOT_HOME
    );

    let alice = users::UserIdentifier::new(1000);
    users_manager
        .add_user(alice, "alice", group, "/users/alice")
        .await
        .unwrap();
    let home = vfs.ensure_user_home(task, alice).await.unwrap();
    assert_eq!(home.as_str(), "/users/alice");
    assert_eq!(vfs.get_statistics(&home).await.unwrap().user, alice);

    // Renaming keeps the recorded home and its contents.
    users_manager.rename_user(alice, "alicia").await.unwrap();
    assert_eq!(
        vfs.ensure_user_home(task, alice).await.unwrap().as_str(),
        "/users/alice"
    );
    assert_eq!(
        vfs.get_user_application_directory(alice, "clock")
            .await
            .unwrap()
            .as_str(),
        "/users/alice/applications/clock"
    );
    assert_eq!(
        vfs.get_user_application_directory(alice, "..").await,
        Err(IoErrorKind::InvalidPath)
    );

    // An account saved before homes were recorded keeps its numeric home.
    let legacy = users::UserIdentifier::new(1001);
    users_manager
        .add_user(legacy, "legacy", group, "/users/1001")
        .await
        .unwrap();
    assert_eq!(
        vfs.ensure_user_home(task, legacy).await.unwrap().as_str(),
        "/users/1001"
    );

    // A recorded home outside /users is never used.
    let escaped = users::UserIdentifier::new(1002);
    users_manager
        .add_user(escaped, "escaped", group, "/system")
        .await
        .unwrap();
    assert_eq!(
        vfs.ensure_user_home(task, escaped).await,
        Err(IoErrorKind::InvalidPath)
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn global_application_storage_follows_posix_and_user_app_storage_is_separate() {
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
        .add_user(user, "test-user", group, "/users/test-user")
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
    assert_eq!(first_home.as_str(), "/users/test-user/applications/clock");
    let user_data = first_home.append("data").unwrap();
    assert_eq!(vfs.get_statistics(&user_data).await.unwrap().user, user);
    assert_ne!(global_directory.as_str(), user_data.as_str());

    let task_manager = vfs.task_manager();
    task_manager.set_user(task, user).await.unwrap();
    // POSIX ownership and mode bits are the only filesystem access policy.
    // The global application data tree is root-owned, so a normal user
    // cannot read it, irrespective of which binary they are running.
    assert_eq!(
        task::block_on(vfs.get_statistics_for_task(task, &global_directory)),
        Err(IoErrorKind::PermissionDenied)
    );
    assert!(matches!(
        File::open(
            vfs,
            task,
            "/data/applications/clock/state",
            AccessFlags::Read.into()
        )
        .await,
        Err(IoErrorKind::PermissionDenied)
    ));

    // A user-owned private directory remains accessible to its owner.
    File::open(
        vfs,
        task,
        user_data.append("state").unwrap().as_str(),
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap()
    .close(vfs)
    .await
    .unwrap();
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
    use virtual_file_system::IoErrorKind;

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
    assert!(matches!(
        trailing_slash_stats,
        Err(IoErrorKind::NotADirectory)
    ));

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
        Err(IoErrorKind::SymlinkLoop)
    ));

    assert!(matches!(
        File::open(
            vfs,
            task,
            "/links/absolute",
            Flags::new(AccessFlags::Read, Some(CreateFlags::CREATE_EXCLUSIVE), None)
        )
        .await,
        Err(IoErrorKind::AlreadyExists)
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
    let options = little_fs::Options::new(256);
    little_fs::FileSystem::format(device, options).unwrap();
    vfs.mount_file_system(
        little_fs::FileSystem::new(device, options).unwrap(),
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
        .add_user(
            user,
            "unprivileged",
            users::GroupIdentifier::ROOT,
            "/users/unprivileged",
        )
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
        Err(IoErrorKind::PermissionDenied)
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
        Err(IoErrorKind::PermissionDenied)
    ));
}

/// A user creating an existing directory under a parent it cannot write, as
/// the graphical shell does with the seeded shortcut directory, is told the
/// directory exists instead of being denied (and logged as an error).
#[cfg(target_os = "linux")]
#[test]
async fn existing_directory_is_reported_before_the_parent_write_check() {
    use virtual_file_system::IoErrorKind;
    let (task, vfs) = initialize().await;
    let user = unprivileged_user(vfs).await;
    vfs.create_directory(task, &"/shared").await.unwrap();
    vfs.create_directory(task, &"/shared/existing")
        .await
        .unwrap();
    File::write_to_path(vfs, task, "/shared/file", b"data")
        .await
        .unwrap();
    vfs.create_symlink(
        task,
        Path::from_str("/missing"),
        Path::from_str("/shared/dangling"),
    )
    .await
    .unwrap();
    // Root-owned and readable by everyone, like the seeded `/configuration/shared`.
    vfs.set_permissions(
        task,
        "/shared",
        file_system::Permissions::from_octal(0o755).unwrap(),
    )
    .await
    .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();

    for path in ["/shared/existing", "/shared/file", "/shared/dangling"] {
        assert_eq!(
            vfs.create_directory(task, &path).await,
            Err(IoErrorKind::AlreadyExists),
            "{path}"
        );
    }
    assert_eq!(
        vfs.create_directory(task, &"/shared/new").await,
        Err(IoErrorKind::PermissionDenied)
    );

    // Without search permission on the parent, existence stays hidden.
    vfs.task_manager()
        .set_user(task, users::UserIdentifier::ROOT)
        .await
        .unwrap();
    vfs.set_permissions(
        task,
        "/shared",
        file_system::Permissions::from_octal(0o700).unwrap(),
    )
    .await
    .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert_eq!(
        vfs.create_directory(task, &"/shared/existing").await,
        Err(IoErrorKind::PermissionDenied)
    );
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
            Err(IoErrorKind::PermissionDenied)
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
async fn shared_and_temporary_directories_allow_user_writes_and_enforce_sticky_bit() {
    use file_system::Permissions;
    use virtual_file_system::IoErrorKind;

    let (task, vfs) = initialize().await;
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();

    let group_a = users::GroupIdentifier::new(1001);
    let user_a = users::UserIdentifier::new(1001);
    let group_b = users::GroupIdentifier::new(1002);
    let user_b = users::UserIdentifier::new(1002);
    vfs.users_manager()
        .add_group(group_a, "alice", &[])
        .await
        .unwrap();
    vfs.users_manager()
        .add_user(user_a, "alice", group_a, "/users/alice")
        .await
        .unwrap();
    vfs.users_manager()
        .add_group(group_b, "bob", &[])
        .await
        .unwrap();
    vfs.users_manager()
        .add_user(user_b, "bob", group_b, "/users/bob")
        .await
        .unwrap();

    // Non-root users can create files in /data/shared, /temporary, and /temporary/cache,
    // while /temporary/sessions stays root-only.
    vfs.task_manager().set_user(task, user_a).await.unwrap();
    File::write_to_path(vfs, task, "/data/shared/alice", b"shared")
        .await
        .unwrap();
    File::write_to_path(vfs, task, "/temporary/alice", b"temp")
        .await
        .unwrap();
    File::write_to_path(vfs, task, "/temporary/cache/alice", b"cache")
        .await
        .unwrap();
    assert!(matches!(
        File::write_to_path(vfs, task, "/temporary/sessions/alice", b"denied").await,
        Err(IoErrorKind::PermissionDenied)
    ));

    // User B can also create files there, but cannot remove, rename, or replace user A's files.
    vfs.task_manager().set_user(task, user_b).await.unwrap();
    File::write_to_path(vfs, task, "/temporary/bob", b"bob")
        .await
        .unwrap();

    for path in ["/data/shared/alice", "/temporary/alice"] {
        assert_eq!(
            vfs.remove(task, path).await,
            Err(IoErrorKind::PermissionDenied)
        );
        assert_eq!(
            vfs.rename(task, &path, &"/temporary/bob_stolen").await,
            Err(IoErrorKind::PermissionDenied)
        );
    }
    assert_eq!(
        vfs.rename(task, &"/temporary/bob", &"/temporary/alice")
            .await,
        Err(IoErrorKind::PermissionDenied)
    );
    assert_eq!(
        vfs.rename(task, &"/temporary/bob", &"/data/shared/alice")
            .await,
        Err(IoErrorKind::PermissionDenied)
    );

    // User B can rename their own file to a non-existent destination.
    vfs.rename(task, &"/temporary/bob", &"/temporary/bob_renamed")
        .await
        .unwrap();

    // User A can rename and remove their own files.
    vfs.task_manager().set_user(task, user_a).await.unwrap();
    vfs.rename(task, &"/data/shared/alice", &"/data/shared/alice_renamed")
        .await
        .unwrap();
    vfs.remove(task, "/data/shared/alice_renamed")
        .await
        .unwrap();

    // Sticky directory owner (non-root) can remove another user's entry inside it.
    vfs.create_directory(task, &"/temporary/alice_dir")
        .await
        .unwrap();
    vfs.set_permissions(task, "/temporary/alice_dir", Permissions::SHARED_DIRECTORY)
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user_b).await.unwrap();
    File::write_to_path(vfs, task, "/temporary/alice_dir/bob", b"b")
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user_a).await.unwrap();
    vfs.remove(task, "/temporary/alice_dir/bob").await.unwrap();
    vfs.remove(task, "/temporary/alice_dir").await.unwrap();

    // Root bypasses sticky bit and can remove/rename any entry.
    vfs.task_manager()
        .set_user(task, users::UserIdentifier::ROOT)
        .await
        .unwrap();
    vfs.rename(task, &"/temporary/bob_renamed", &"/temporary/alice")
        .await
        .unwrap();
    vfs.remove(task, "/temporary/alice").await.unwrap();
    vfs.remove(task, "/temporary/cache/alice").await.unwrap();

    // Repeated create_default_hierarchy keeps SHARED_DIRECTORY permissions.
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();
    for path in ["/data/shared", "/temporary", "/temporary/cache"] {
        assert_eq!(
            vfs.get_statistics(&Path::from_str(path))
                .await
                .unwrap()
                .permissions,
            Permissions::SHARED_DIRECTORY
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
        Err(IoErrorKind::InvalidArgument)
    );
    assert!(matches!(
        vfs.get_statistics(&"/zero").await,
        Err(IoErrorKind::NotFound)
    ));
}

#[cfg(target_os = "linux")]
async fn mount_test_filesystem(vfs: &'static VirtualFileSystem, task: TaskIdentifier, path: &str) {
    let device = MemoryDevice::<512>::new_static(512 * 128);
    let options = little_fs::Options::new(256);
    little_fs::FileSystem::format(device, options).unwrap();
    vfs.mount_file_system(
        little_fs::FileSystem::new(device, options).unwrap(),
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
        Err(IoErrorKind::ResourceBusy)
    );
    assert_eq!(
        vfs.remove(task, "/ancestor/mount").await,
        Err(IoErrorKind::ResourceBusy)
    );
    assert_eq!(
        vfs.rename(task, &"/ancestor/mount", &"/other").await,
        Err(IoErrorKind::ResourceBusy)
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
            Err(IoErrorKind::ResourceBusy)
        );
    }
    vfs.unmount("/parent/child", false).await.unwrap();
    vfs.unmount("/parent", false).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn invalid_or_pre_epoch_utc_does_not_become_unsigned_file_metadata() {
    let time_manager = Box::leak(Box::new(time::Manager::unsynchronized()));
    let (task, vfs) = initialize_with_time(time_manager).await;
    let flags = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    assert!(matches!(
        File::open(vfs, task, "/invalid-time", flags).await,
        Err(IoErrorKind::DeviceUnavailable)
    ));
    time_manager
        .set_realtime(time::RealtimeSample {
            timestamp: time::Timestamp::from_nanos(-10_000_000_000),
            source: time::ClockSource::Manual,
            resolution: core::time::Duration::from_secs(1),
            accuracy: None,
        })
        .unwrap();
    assert!(matches!(
        File::open(vfs, task, "/invalid-time", flags).await,
        Err(IoErrorKind::Overflow)
    ));
    assert!(matches!(
        vfs.get_statistics_for_task(task, &"/invalid-time").await,
        Err(IoErrorKind::NotFound)
    ));
    time_manager
        .set_realtime(time::RealtimeSample {
            timestamp: time::Timestamp::from_nanos(1_000_000_000),
            source: time::ClockSource::Manual,
            resolution: core::time::Duration::from_secs(1),
            accuracy: None,
        })
        .unwrap();
    File::open(vfs, task, "/invalid-time", flags).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn existing_files_remain_accessible_without_representable_utc() {
    let clock = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let (task, vfs) = initialize_with_time(clock).await;
    let mut file = File::open(
        vfs,
        task,
        "/existing",
        Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    file.write(b"epoch-independent data").await.unwrap();
    drop(file);
    vfs.create_symlink(task, Path::from_str("/existing"), Path::from_str("/link"))
        .await
        .unwrap();
    let before = vfs
        .get_statistics_for_task(task, &"/existing")
        .await
        .unwrap();
    for invalid in [false, true] {
        if invalid {
            clock.invalidate();
        } else {
            clock
                .set_realtime(time::RealtimeSample {
                    timestamp: time::Timestamp::from_nanos(-10_000_000_000),
                    source: time::ClockSource::Manual,
                    resolution: core::time::Duration::from_secs(1),
                    accuracy: None,
                })
                .unwrap();
        }
        let stats = vfs
            .get_statistics_for_task(task, &"/existing")
            .await
            .unwrap();
        assert_eq!(stats.access, before.access);
        assert_eq!(stats.modification, before.modification);
        let mut file = File::open(vfs, task, "/link", Flags::from(AccessFlags::Read))
            .await
            .unwrap();
        let mut data = [0; 22];
        assert_eq!(file.read(&mut data).await.unwrap(), 22);
        assert_eq!(&data, b"epoch-independent data");
        drop(file);
        let mut writable = File::open(vfs, task, "/existing", Flags::from(AccessFlags::READ_WRITE))
            .await
            .unwrap();
        writable.write(b"epoch-independent data").await.unwrap();
        drop(writable);
        let mut target = [0; 32];
        let size = vfs
            .read_symlink(task, Path::from_str("/link"), &mut target)
            .await
            .unwrap();
        assert_eq!(&target[..size], b"/existing");
        drop(vfs.open_directory(task, &"/").await.unwrap());
        let stats = vfs
            .get_statistics_for_task(task, &"/existing")
            .await
            .unwrap();
        assert_eq!(stats.access, before.access);
        assert_eq!(stats.modification, before.modification);
        vfs.rename(task, &"/existing", &"/renamed").await.unwrap();
        vfs.rename(task, &"/renamed", &"/existing").await.unwrap();
    }
    vfs.remove(task, "/link").await.unwrap();
    vfs.remove(task, "/existing").await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn confined_operations_reject_a_replaced_root_or_root_ancestor() {
    let (task, virtual_file_system) = initialize().await;
    for path in ["/granted", "/granted/child", "/other", "/other/child"] {
        virtual_file_system
            .create_directory(task, &path)
            .await
            .unwrap();
    }
    File::write_to_path(
        virtual_file_system,
        task,
        "/granted/child/value",
        b"granted",
    )
    .await
    .unwrap();
    File::write_to_path(virtual_file_system, task, "/other/child/value", b"other")
        .await
        .unwrap();

    let relative = Path::new("value");
    let root = Path::new("/granted/child");
    let flags = Flags::from(AccessFlags::Read);
    assert!(
        virtual_file_system
            .open_beneath(&relative, flags, task, root)
            .await
            .is_ok()
    );

    virtual_file_system
        .rename(task, &"/granted", &"/original")
        .await
        .unwrap();
    virtual_file_system
        .create_symlink(task, Path::new("/other"), Path::new("/granted"))
        .await
        .unwrap();
    assert!(matches!(
        virtual_file_system
            .open_beneath(&relative, flags, task, root)
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    ));

    virtual_file_system.remove(task, "/granted").await.unwrap();
    virtual_file_system
        .rename(task, &"/original", &"/granted")
        .await
        .unwrap();
    virtual_file_system
        .rename(task, &"/granted/child", &"/granted/original")
        .await
        .unwrap();
    virtual_file_system
        .create_symlink(task, Path::new("../other/child"), root)
        .await
        .unwrap();
    assert!(matches!(
        virtual_file_system
            .open_beneath(&relative, flags, task, root)
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    ));
}

#[cfg(target_os = "linux")]
#[test]
async fn confined_operations_do_not_dispatch_to_backends_without_namespace_guarantees() {
    let (task, virtual_file_system) = initialize().await;
    let path = Path::new("/sandbox/mounted");
    virtual_file_system
        .create_directory(task, &"/sandbox")
        .await
        .unwrap();
    virtual_file_system
        .create_directory(task, &path)
        .await
        .unwrap();
    // The dummy backend panics on namespace operations, so this verifies
    // rejection before the VFS delegates any part of a confined lookup.
    virtual_file_system
        .mount_file_system(file_system::DummyFileSystem, path, task)
        .await
        .unwrap();
    assert_eq!(
        virtual_file_system
            .get_statistics_beneath(task, &Path::new("mounted"), true, Path::new("/sandbox"))
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    );
    assert_eq!(
        virtual_file_system
            .get_statistics_beneath(task, &Path::new("."), true, path)
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn confined_namespace_changes_cannot_target_the_resolved_root() {
    let (task, virtual_file_system) = initialize().await;
    let root = Path::new("/sandbox");
    virtual_file_system
        .create_directory(task, &root)
        .await
        .unwrap();
    virtual_file_system
        .create_symlink(task, Path::new("."), Path::new("/sandbox/alias"))
        .await
        .unwrap();
    let root_alias = Path::new("alias/.");
    assert_eq!(
        virtual_file_system
            .remove_beneath(task, root_alias, root)
            .await,
        Err(IoErrorKind::ResourceBusy)
    );
    assert_eq!(
        virtual_file_system
            .rename_beneath(task, &root_alias, &Path::new("moved"), root, Path::ROOT)
            .await,
        Err(IoErrorKind::ResourceBusy)
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn confined_open_can_decline_to_follow_the_final_link() {
    let (task, virtual_file_system) = initialize().await;
    let root = Path::new("/sandbox");
    virtual_file_system
        .create_directory(task, &root)
        .await
        .unwrap();
    virtual_file_system
        .create_directory(task, &"/sandbox/subdirectory")
        .await
        .unwrap();
    File::write_to_path(
        virtual_file_system,
        task,
        "/sandbox/subdirectory/value",
        b"data",
    )
    .await
    .unwrap();
    for (name, target) in [("alias", "subdirectory"), ("dangling", "missing")] {
        let link = format!("/sandbox/{name}");
        virtual_file_system
            .create_symlink(task, Path::new(target), Path::new(&link))
            .await
            .unwrap();
    }

    let flags = Flags::from(AccessFlags::Read);
    let dangling = Path::new("dangling");
    assert!(matches!(
        virtual_file_system
            .open_beneath_with_lookup(&dangling, flags, task, root, false)
            .await,
        Err(IoErrorKind::SymlinkLoop)
    ));
    assert!(matches!(
        virtual_file_system
            .open_beneath_with_lookup(&dangling, flags, task, root, true)
            .await,
        Err(IoErrorKind::NotFound)
    ));
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    assert!(matches!(
        virtual_file_system
            .open_beneath_with_lookup(&dangling, create, task, root, false)
            .await,
        Err(IoErrorKind::SymlinkLoop)
    ));
    let exclusive = create.set_open(CreateFlags::CREATE_EXCLUSIVE);
    for follow in [false, true] {
        assert!(matches!(
            virtual_file_system
                .open_beneath_with_lookup(&dangling, exclusive, task, root, follow)
                .await,
            Err(IoErrorKind::AlreadyExists)
        ));
    }
    let mut target = [0; 16];
    let length = virtual_file_system
        .read_symlink_beneath(task, dangling, &mut target, root)
        .await
        .unwrap();
    assert_eq!(&target[..length], b"missing");
    assert_eq!(
        virtual_file_system
            .get_statistics_beneath(task, &Path::new("missing"), false, root)
            .await,
        Err(IoErrorKind::NotFound)
    );

    assert!(matches!(
        virtual_file_system
            .open_directory_beneath_with_lookup(task, &Path::new("alias"), root, false)
            .await,
        Err(IoErrorKind::SymlinkLoop)
    ));
    assert!(
        virtual_file_system
            .open_directory_beneath_with_lookup(task, &Path::new("alias"), root, true)
            .await
            .is_ok()
    );
    assert!(
        virtual_file_system
            .open_beneath_with_lookup(&Path::new("alias/value"), flags, task, root, false)
            .await
            .is_ok()
    );
}

#[cfg(target_os = "linux")]
#[test]
async fn beneath_resolution_confines_links_and_all_namespace_operations() {
    use virtual_file_system::IoErrorKind;
    let (task, vfs) = initialize().await;
    let root = Path::new("/sandbox");
    for path in ["/sandbox", "/sandbox/sub", "/outside"] {
        vfs.create_directory(task, &path).await.unwrap();
    }
    for (name, target) in [
        ("absolute", "/outside"),
        ("parent", "../outside"),
        ("chain", "absolute"),
        ("inside", "sub"),
        ("sub/up", ".."),
        ("loop", "loop"),
    ] {
        let link = format!("/sandbox/{name}");
        vfs.create_symlink(task, Path::new(target), Path::new(&link))
            .await
            .unwrap();
    }
    vfs.set_times_beneath(
        task,
        &Path::new("sub"),
        true,
        root,
        Some(file_system::Time::new(2)),
        Some(file_system::Time::new(3)),
    )
    .await
    .unwrap();
    let stats = vfs
        .get_statistics_beneath(task, &Path::new("sub"), true, root)
        .await
        .unwrap();
    assert_eq!(stats.modification, file_system::Time::new(3));
    for path in ["absolute", "parent", "../outside", "chain/file"] {
        assert_eq!(
            vfs.set_times_beneath(
                task,
                &Path::new(path),
                true,
                root,
                Some(file_system::Time::new(4)),
                None
            )
            .await,
            Err(IoErrorKind::PathEscapesRoot)
        );
    }
    for path in [
        "absolute",
        "parent",
        "chain",
        "../outside",
        "sub/up/../outside",
        "inside/../../outside",
    ] {
        assert_eq!(
            vfs.get_statistics_beneath(task, &Path::new(path), true, root)
                .await,
            Err(IoErrorKind::PathEscapesRoot),
            "{path}"
        );
    }
    for path in ["inside", "sub/up/sub", "inside/../sub"] {
        assert_eq!(
            vfs.get_statistics_beneath(task, &Path::new(path), true, root)
                .await
                .unwrap()
                .kind,
            file_system::Kind::Directory
        );
    }
    assert_eq!(
        vfs.get_statistics_beneath(task, &Path::new("absolute"), false, root)
            .await
            .unwrap()
            .kind,
        file_system::Kind::SymbolicLink
    );
    assert!(matches!(
        vfs.get_statistics_beneath(task, &Path::new("loop"), true, root)
            .await,
        Err(IoErrorKind::SymlinkLoop)
    ));
    assert!(
        vfs.get_statistics_for_task(task, &"/sandbox/absolute")
            .await
            .is_ok()
    );
    assert!(
        vfs.get_statistics_for_task(task, &"/sandbox/parent")
            .await
            .is_ok()
    );
    assert!(matches!(
        vfs.open_beneath(
            &Path::new("absolute/file"),
            Flags::from(AccessFlags::Read),
            task,
            root
        )
        .await,
        Err(IoErrorKind::PathEscapesRoot)
    ));
    assert!(matches!(
        vfs.open_directory_beneath(task, &Path::new("absolute"), root)
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    ));
    assert_eq!(
        vfs.create_directory_beneath(task, &Path::new("absolute/new"), root)
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    );
    assert_eq!(
        vfs.remove_beneath(task, Path::new("absolute/file"), root)
            .await,
        Err(IoErrorKind::PathEscapesRoot)
    );
    assert_eq!(
        vfs.rename_beneath(
            task,
            &Path::new("sub"),
            &Path::new("absolute/new"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::PathEscapesRoot)
    );
    assert_eq!(
        vfs.rename_beneath(
            task,
            &Path::new("absolute/file"),
            &Path::new("new"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::PathEscapesRoot)
    );
    vfs.create_symlink_beneath(task, Path::new("sub"), Path::new("newlink"), root)
        .await
        .unwrap();
    let mut target = [0; 16];
    assert_eq!(
        vfs.read_symlink_beneath(task, Path::new("newlink"), &mut target, root)
            .await
            .unwrap(),
        3
    );
    mount_test_filesystem(vfs, task, "/sandbox/mounted").await;
    vfs.create_directory_beneath(task, &Path::new("mounted/dir"), root)
        .await
        .unwrap();
    vfs.create_symlink_beneath(task, Path::new("mounted/dir"), Path::new("mountlink"), root)
        .await
        .unwrap();
    assert!(
        vfs.open_directory_beneath(task, &Path::new("mountlink"), root)
            .await
            .is_ok()
    );
    let allocations = allocation::measure(|| {
        task::block_on(vfs.get_statistics_beneath(task, &Path::new("sub"), true, root)).unwrap();
    });
    let baseline = allocation::measure(|| {
        task::block_on(vfs.get_statistics_for_task(task, &"/sandbox/sub")).unwrap();
    });
    std::println!(
        "canonical statistics allocator requests: beneath={allocations}, unconfined={baseline}"
    );
    let mut file = vfs
        .open_beneath(
            &Path::new("file"),
            Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
            task,
            root,
        )
        .await
        .unwrap();
    let writes = allocation::measure(|| {
        task::block_on(file.write(b"x")).unwrap();
    });
    let mut byte = [0];
    let reads = allocation::measure(|| {
        task::block_on(file.read(&mut byte)).unwrap();
    });
    std::println!("confined handle allocator requests: read={reads}, write={writes}");
    assert_eq!((reads, writes), (0, 0));
}

#[cfg(target_os = "linux")]
#[test]
async fn hard_links_stay_beneath_their_roots_and_report_missing_backend_support() {
    use virtual_file_system::IoErrorKind;
    let (task, vfs) = initialize().await;
    let root = Path::new("/sandbox");
    for path in ["/sandbox", "/sandbox/dir", "/outside"] {
        vfs.create_directory(task, &path).await.unwrap();
    }
    File::write_to_path(vfs, task, "/sandbox/file", b"data")
        .await
        .unwrap();
    for (name, target) in [("absolute", "/outside"), ("parent", "../outside")] {
        let link = format!("/sandbox/{name}");
        vfs.create_symlink(task, Path::new(target), Path::new(&link))
            .await
            .unwrap();
    }
    for (existing, new) in [
        ("absolute/file", "new"),
        ("parent/file", "new"),
        ("../outside", "new"),
        ("/outside", "new"),
        ("file", "absolute/new"),
        ("file", "../new"),
        ("file", "/new"),
    ] {
        assert_eq!(
            vfs.link_beneath(
                task,
                &Path::new(existing),
                true,
                &Path::new(new),
                root,
                root
            )
            .await,
            Err(IoErrorKind::PathEscapesRoot),
            "{existing} -> {new}"
        );
    }
    assert!(matches!(
        vfs.link_beneath(
            task,
            &Path::new("missing"),
            true,
            &Path::new("new"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::NotFound)
    ));
    assert!(matches!(
        vfs.link_beneath(task, &Path::new("dir"), true, &Path::new("new"), root, root)
            .await,
        Err(IoErrorKind::IsADirectory)
    ));
    assert_eq!(
        vfs.link_beneath(
            task,
            &Path::new("file"),
            true,
            &Path::new("dir"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::AlreadyExists)
    );
    // LittleFS has no hard links: a valid request must say so instead of
    // pretending to succeed or silently copying.
    assert_eq!(
        vfs.link_beneath(
            task,
            &Path::new("file"),
            true,
            &Path::new("new"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::Unsupported)
    );
    assert!(
        vfs.get_statistics_beneath(task, &Path::new("new"), false, root)
            .await
            .is_err()
    );
    // A directory cannot be renamed into its own subtree, nor onto itself plus a suffix
    // that merely shares its name prefix.
    vfs.create_directory_beneath(task, &Path::new("dir/inner"), root)
        .await
        .unwrap();
    assert_eq!(
        vfs.rename_beneath(
            task,
            &Path::new("dir"),
            &Path::new("dir/inner/moved"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::InvalidArgument)
    );
    assert_eq!(
        vfs.rename_beneath(task, &Path::new("dir"), &Path::new("dir/moved"), root, root)
            .await,
        Err(IoErrorKind::InvalidArgument)
    );
    vfs.rename_beneath(task, &Path::new("dir"), &Path::new("dir2"), root, root)
        .await
        .unwrap();
    vfs.rename_beneath(task, &Path::new("dir2"), &Path::new("dir"), root, root)
        .await
        .unwrap();
    assert!(
        vfs.get_statistics_beneath(task, &Path::new("dir/inner"), false, root)
            .await
            .is_ok()
    );
    // Neither a rename nor a link may cross a mount boundary.
    mount_test_filesystem(vfs, task, "/sandbox/mounted").await;
    assert_eq!(
        vfs.link_beneath(
            task,
            &Path::new("file"),
            true,
            &Path::new("mounted/new"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::CrossDevice)
    );
    assert_eq!(
        vfs.rename_beneath(
            task,
            &Path::new("file"),
            &Path::new("mounted/new"),
            root,
            root
        )
        .await,
        Err(IoErrorKind::CrossDevice)
    );
    assert_eq!(
        vfs.get_statistics_beneath(task, &Path::new("file"), false, root)
            .await
            .unwrap()
            .size,
        4
    );
}

/// Memory block device whose flush can be made to fail.
struct FlakyDevice {
    inner: &'static MemoryDevice<512>,
    fail_flush: std::sync::atomic::AtomicBool,
}

impl file_system::DirectBaseOperations for FlakyDevice {
    fn read(
        &self,
        cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        self.inner.read(cx, buffer, position)
    }

    fn write(
        &self,
        cx: &mut core::task::Context<'_>,
        buffer: &[u8],
        position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        self.inner.write(cx, buffer, position)
    }

    fn flush(&self, cx: &mut core::task::Context<'_>) -> core::task::Poll<file_system::Result<()>> {
        if self.fail_flush.load(std::sync::atomic::Ordering::SeqCst) {
            return core::task::Poll::Ready(Err(IoErrorKind::InputOutput));
        }
        self.inner.flush(cx)
    }

    fn control(
        &self,
        cx: &mut core::task::Context<'_>,
        command: file_system::ControlCommandIdentifier,
        input: &shared::AnyByLayout,
        output: &mut shared::AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        self.inner.control(cx, command, input, output)
    }
}

impl file_system::MountOperations for FlakyDevice {}
impl file_system::DirectBlockDevice for FlakyDevice {}

async fn initialize_on(
    device: &'static FlakyDevice,
) -> (TaskIdentifier, &'static VirtualFileSystem) {
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    if !log::is_initialized() {
        log::initialize(&testing_support::Logger).unwrap();
    }
    let task = task_manager.get_current_task_identifier().await;
    let file_system =
        little_fs::FileSystem::get_or_format(device, little_fs::Options::new(256)).unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users_manager, time_manager, file_system)
            .unwrap(),
    ));
    (task, vfs)
}

fn flaky_device() -> &'static FlakyDevice {
    Box::leak(Box::new(FlakyDevice {
        inner: MemoryDevice::<512>::new_static(1024 * 512),
        fail_flush: std::sync::atomic::AtomicBool::new(false),
    }))
}

#[cfg(target_os = "linux")]
#[test]
async fn storage_shutdown_requires_closed_handles_and_freezes_io() {
    let (task, vfs) = initialize().await;
    let mut file = File::open(
        vfs,
        task,
        "/data",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    file.write(b"durable").await.unwrap();
    assert!(matches!(
        vfs.shutdown_storage().await,
        Err(IoErrorKind::ResourceBusy)
    ));
    // A refused shutdown leaves storage fully usable.
    file.write(b" data").await.unwrap();
    file.close(vfs).await.unwrap();

    vfs.shutdown_storage().await.unwrap();
    assert!(
        File::write_to_path(vfs, task, "/late", b"forbidden")
            .await
            .is_err()
    );
    assert!(
        File::open(vfs, task, "/data", AccessFlags::Write.into())
            .await
            .is_err()
    );
    assert!(vfs.create_directory(task, &"/late").await.is_err());
    assert!(vfs.remove(task, "/data").await.is_err());
    assert!(
        File::open(vfs, task, "/data", AccessFlags::Read.into())
            .await
            .is_err()
    );
    // Shutdown runs once; a second attempt does not unmount again.
    assert!(vfs.shutdown_storage().await.is_ok());
}

#[cfg(target_os = "linux")]
#[test]
async fn synchronous_handles_support_offset_io_timestamps_and_status_flags() {
    let (task, vfs) = initialize().await;
    let mut file = vfs
        .open(
            &"/file",
            Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
            task,
        )
        .await
        .unwrap()
        .into_synchronous_file();
    assert_eq!(file.write(b"0123456789").unwrap(), 10);
    file.set_position(&Position::Start(2)).unwrap();

    // Offset I/O must neither use nor move the shared cursor, and the cursor
    // reported afterwards must not be the backend's private position.
    assert_eq!(file.write_at(b"ab", 8).unwrap(), 2);
    assert_eq!(file.set_position(&Position::Current(0)).unwrap(), 2);
    let mut bytes = [0; 4];
    assert_eq!(file.read_at(&mut bytes, 6).unwrap(), 4);
    assert_eq!(&bytes, b"67ab");
    assert_eq!(file.set_position(&Position::Current(0)).unwrap(), 2);
    assert_eq!(file.set_position(&Position::Current(3)).unwrap(), 5);
    assert_eq!(file.set_position(&Position::Current(-5)).unwrap(), 0);
    assert!(file.set_position(&Position::Current(-1)).is_err());
    assert_eq!(file.set_position(&Position::Current(0)).unwrap(), 0);

    // Handle timestamps: None leaves a field untouched.
    file.set_times(Some(file_system::Time::new(7)), None)
        .unwrap();
    let statistics = file.get_statistics().unwrap();
    assert_eq!(statistics.access, file_system::Time::new(7));
    file.set_times(None, Some(file_system::Time::new(9)))
        .unwrap();
    let statistics = file.get_statistics().unwrap();
    assert_eq!(statistics.access, file_system::Time::new(7));
    assert_eq!(statistics.modification, file_system::Time::new(9));

    // Status flags: append moves every write to the end of the file.
    file.set_state(StateFlags::Append).unwrap();
    assert_eq!(file.get_state().unwrap(), StateFlags::Append);
    file.set_position(&Position::Start(0)).unwrap();
    assert_eq!(file.write(b"Z").unwrap(), 1);
    assert_eq!(file.get_statistics().unwrap().size, 11);
    file.set_state(StateFlags::empty()).unwrap();
    file.set_position(&Position::Start(0)).unwrap();
    file.write(b"Y").unwrap();
    assert_eq!(file.get_statistics().unwrap().size, 11);
    assert!(file.set_state(StateFlags::Synchronous).is_err());
}

#[cfg(target_os = "linux")]
#[test]
async fn asynchronous_writes_honour_append_set_after_opening() {
    let (task, vfs) = initialize().await;
    let mut file = vfs
        .open(
            &"/append",
            Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
            task,
        )
        .await
        .unwrap();
    assert_eq!(file.write(b"0123456789").await.unwrap(), 10);

    // Append is set on the open handle, then the asynchronous path writes.
    let mut synchronous = file.into_synchronous_file();
    synchronous.set_state(StateFlags::Append).unwrap();
    let mut file = File::from_synchronous_file(synchronous);
    file.set_position(&Position::Start(0)).await.unwrap();
    assert_eq!(file.write(b"Z").await.unwrap(), 1);
    assert_eq!(file.get_statistics().await.unwrap().size, 11);

    // Without Append, the write lands at the cursor and the size is unchanged.
    let mut synchronous = file.into_synchronous_file();
    synchronous.set_state(StateFlags::empty()).unwrap();
    let mut file = File::from_synchronous_file(synchronous);
    file.set_position(&Position::Start(0)).await.unwrap();
    assert_eq!(file.write(b"Y").await.unwrap(), 1);
    assert_eq!(file.get_statistics().await.unwrap().size, 11);
    file.close(vfs).await.unwrap();
}

#[cfg(target_os = "linux")]
#[test]
async fn little_fs_data_survives_storage_shutdown_and_remount() {
    let device = flaky_device();
    {
        let (task, vfs) = initialize_on(device).await;
        vfs.create_directory(task, &"/configuration").await.unwrap();
        File::write_to_path(vfs, task, "/configuration/settings", b"kept")
            .await
            .unwrap();
        vfs.shutdown_storage().await.unwrap();
    }

    let (task, vfs) = initialize_on(device).await;
    let mut buffer = Vec::new();
    File::read_from_path(vfs, task, "/configuration/settings", &mut buffer)
        .await
        .unwrap();
    assert_eq!(buffer, b"kept");
}

#[cfg(target_os = "linux")]
#[test]
async fn storage_flush_failure_is_surfaced_and_keeps_the_mount() {
    let device = flaky_device();
    let (task, vfs) = initialize_on(device).await;
    File::write_to_path(vfs, task, "/pending", b"data")
        .await
        .unwrap();
    device
        .fail_flush
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(vfs.shutdown_storage().await.is_err());
    // Writes stay frozen, but the filesystem was not torn down: once the
    // device recovers, committed data is still readable for diagnosis.
    assert!(
        File::write_to_path(vfs, task, "/after", b"x")
            .await
            .is_err()
    );
    device
        .fail_flush
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let mut buffer = Vec::new();
    File::read_from_path(vfs, task, "/pending", &mut buffer)
        .await
        .unwrap();
    assert_eq!(buffer, b"data");
}

struct CountingBlockDevice {
    inner: &'static MemoryDevice<512>,
    write_count: std::sync::atomic::AtomicUsize,
}

impl CountingBlockDevice {
    fn new_static() -> &'static Self {
        Box::leak(Box::new(Self {
            inner: MemoryDevice::<512>::new_static(1024 * 512),
            write_count: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    fn write_count(&self) -> usize {
        self.write_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl file_system::DirectBaseOperations for CountingBlockDevice {
    fn read(
        &self,
        task_context: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        self.inner.read(task_context, buffer, absolute_position)
    }

    fn write(
        &self,
        task_context: &mut core::task::Context<'_>,
        buffer: &[u8],
        absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        self.write_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.write(task_context, buffer, absolute_position)
    }

    fn flush(
        &self,
        task_context: &mut core::task::Context<'_>,
    ) -> core::task::Poll<file_system::Result<()>> {
        self.inner.flush(task_context)
    }

    fn control(
        &self,
        task_context: &mut core::task::Context<'_>,
        command: file_system::ControlCommandIdentifier,
        input: &shared::AnyByLayout,
        output: &mut shared::AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        self.inner.control(task_context, command, input, output)
    }
}

impl file_system::MountOperations for CountingBlockDevice {}
impl file_system::DirectBlockDevice for CountingBlockDevice {}

struct LoopbackCharacterDevice {
    buffer: synchronization::mutex::Mutex<
        synchronization::blocking_mutex::raw::CriticalSectionRawMutex,
        Vec<u8>,
    >,
}

impl LoopbackCharacterDevice {
    const fn new() -> Self {
        Self {
            buffer: synchronization::mutex::Mutex::new(Vec::new()),
        }
    }
}

impl file_system::DirectBaseOperations for LoopbackCharacterDevice {
    fn read(
        &self,
        _task_context: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let mut stored = self
                .buffer
                .try_lock()
                .map_err(|_| IoErrorKind::WouldBlock)?;
            let read_length = buffer.len().min(stored.len());
            buffer[..read_length].copy_from_slice(&stored[..read_length]);
            stored.drain(..read_length);
            Ok(read_length)
        })
    }

    fn write(
        &self,
        _task_context: &mut core::task::Context<'_>,
        buffer: &[u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            let mut stored = self
                .buffer
                .try_lock()
                .map_err(|_| IoErrorKind::WouldBlock)?;
            stored.extend_from_slice(buffer);
            Ok(buffer.len())
        })
    }
}

impl file_system::MountOperations for LoopbackCharacterDevice {}
impl file_system::DirectCharacterDevice for LoopbackCharacterDevice {}

static STATIC_SERIAL_DEVICE: LoopbackCharacterDevice = LoopbackCharacterDevice::new();

#[cfg(target_os = "linux")]
#[test]
async fn embedded_boot_writes_nothing_after_a_reset() {
    use executable::{ExecutableContext, ExecutableTrait, MainFuture, Standard, mount_executables};
    use virtual_file_system::{AccessTimePolicy, MountOptions};

    struct BootExecutable;

    impl ExecutableTrait for BootExecutable {
        fn main(_standard: Standard<'static>, _arguments: Vec<String>) -> MainFuture {
            Box::pin(async { Ok(()) })
        }
    }

    const EXECUTABLE_PATH: &str = "/system/executables/boot_executable";
    const NULL_DEVICE_PATH: &str = "/devices/standard_io/null";
    const PERSISTENT_EXECUTABLE_PATH: &str = "/applications/executables/persistent.wasm";

    let time_manager = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let task_manager = task::get_instance();
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    if !log::is_initialized() {
        log::initialize(&testing_support::Logger).unwrap();
    }
    let task = task_manager.get_current_task_identifier().await;
    let counting_device = CountingBlockDevice::new_static();
    little_fs::FileSystem::format(counting_device, little_fs::Options::new(256)).unwrap();

    let mut writes = [0; 3];
    for (boot, writes_this_boot) in writes.iter_mut().enumerate() {
        let before = counting_device.write_count();
        // A reset starts with fresh mounts over the same block device.
        let root = little_fs::FileSystem::new(
            counting_device,
            little_fs::Options::new(256).set_block_cycles(500),
        )
        .unwrap();
        let virtual_file_system = Box::leak(Box::new(
            virtual_file_system::initialize_with_options(
                task_manager,
                users_manager,
                time_manager,
                root,
                MountOptions::new().with_access_time_policy(AccessTimePolicy::Never),
            )
            .unwrap(),
        ));
        virtual_file_system
            .mount_file_system(memory_file_system::FileSystem::new(), Path::DEVICES, task)
            .await
            .unwrap();
        virtual_file_system::create_default_hierarchy(virtual_file_system, task)
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(
                memory_file_system::FileSystem::new(),
                Path::SYSTEM_EXECUTABLES,
                task,
            )
            .await
            .unwrap();
        virtual_file_system::clear_temporary(virtual_file_system, task)
            .await
            .unwrap();
        virtual_file_system
            .mount_character_device(task, NULL_DEVICE_PATH, drivers_core::NullDevice)
            .await
            .unwrap();
        let context = Box::leak(Box::new(ExecutableContext::new(
            task_manager,
            users_manager,
            virtual_file_system,
            time_manager,
        )));
        mount_executables!(
            context,
            virtual_file_system,
            task,
            &[(EXECUTABLE_PATH, BootExecutable)]
        )
        .await
        .unwrap();
        let standard = Standard::open(
            &NULL_DEVICE_PATH,
            &NULL_DEVICE_PATH,
            &NULL_DEVICE_PATH,
            task,
            virtual_file_system,
            context,
        )
        .await
        .unwrap();
        let executable = executable::execute(context, EXECUTABLE_PATH, Vec::new(), standard, None)
            .await
            .unwrap();
        assert_eq!(executable.join().await, 0);

        if boot == 0 {
            File::write_to_path(
                virtual_file_system,
                task,
                PERSISTENT_EXECUTABLE_PATH,
                b"persistent executable contents",
            )
            .await
            .unwrap();
        } else {
            let mut contents = Vec::new();
            File::read_from_path(
                virtual_file_system,
                task,
                PERSISTENT_EXECUTABLE_PATH,
                &mut contents,
            )
            .await
            .unwrap();
            assert_eq!(contents, b"persistent executable contents");
        }
        *writes_this_boot = counting_device.write_count() - before;
    }
    assert_eq!(writes[1], 0, "second boot wrote: {writes:?}");
    assert_eq!(writes[2], 0, "third boot wrote: {writes:?}");
}

#[cfg(target_os = "linux")]
#[test]
async fn default_hierarchy_over_memory_devices_writes_nothing_on_second_boot() {
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    if !log::is_initialized() {
        log::initialize(&testing_support::Logger).unwrap();
    }

    let task = task_manager.get_current_task_identifier().await;
    let counting_device = CountingBlockDevice::new_static();
    let root_file_system =
        little_fs::FileSystem::get_or_format(counting_device, little_fs::Options::new(256))
            .unwrap();
    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(
            task_manager,
            users_manager,
            time_manager,
            root_file_system,
        )
        .unwrap(),
    ));

    // The order a platform boots in: devices in memory, then the hierarchy,
    // which creates the device subdirectories inside the memory file system.
    virtual_file_system
        .mount_file_system(memory_file_system::FileSystem::new(), Path::DEVICES, task)
        .await
        .unwrap();
    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();
    assert_eq!(
        virtual_file_system
            .get_statistics(&Path::STANDARD_IO_DEVICES)
            .await
            .unwrap()
            .kind,
        file_system::Kind::Directory
    );

    let writes_after_first_boot = counting_device.write_count();
    virtual_file_system::create_default_hierarchy(virtual_file_system, task)
        .await
        .unwrap();
    assert_eq!(counting_device.write_count(), writes_after_first_boot);
}

#[cfg(target_os = "linux")]
#[test]
async fn memory_file_system_mounted_on_devices_avoids_little_fs_writes() {
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    if !log::is_initialized() {
        log::initialize(&testing_support::Logger).unwrap();
    }

    let task = task_manager.get_current_task_identifier().await;
    let counting_device = CountingBlockDevice::new_static();
    let root_file_system =
        little_fs::FileSystem::get_or_format(counting_device, little_fs::Options::new(256))
            .unwrap();
    let virtual_file_system = Box::leak(Box::new(
        virtual_file_system::initialize(
            task_manager,
            users_manager,
            time_manager,
            root_file_system,
        )
        .unwrap(),
    ));

    virtual_file_system
        .mount_file_system(memory_file_system::FileSystem::new(), Path::DEVICES, task)
        .await
        .unwrap();

    let writes_after_mount = counting_device.write_count();

    virtual_file_system
        .create_directory(task, &Path::UART_DEVICES)
        .await
        .unwrap();
    virtual_file_system
        .mount_character_device(
            task,
            Path::from_str("/devices/uart/uart0"),
            LoopbackCharacterDevice::new(),
        )
        .await
        .unwrap();
    virtual_file_system::mount_static!(
        virtual_file_system,
        task,
        &[("/devices/serial", CharacterDevice, STATIC_SERIAL_DEVICE)]
    )
    .await
    .unwrap();

    let mut uart_file = File::open(
        virtual_file_system,
        task,
        "/devices/uart/uart0",
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .unwrap();
    assert_eq!(uart_file.write(b"ping").await.unwrap(), 4);
    let mut uart_buffer = [0u8; 4];
    assert_eq!(uart_file.read(&mut uart_buffer).await.unwrap(), 4);
    assert_eq!(&uart_buffer, b"ping");
    uart_file.close(virtual_file_system).await.unwrap();

    let mut serial_file = File::open(
        virtual_file_system,
        task,
        "/devices/serial",
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .unwrap();
    assert_eq!(serial_file.write(b"pong").await.unwrap(), 4);
    let mut serial_buffer = [0u8; 4];
    assert_eq!(serial_file.read(&mut serial_buffer).await.unwrap(), 4);
    assert_eq!(&serial_buffer, b"pong");
    serial_file.close(virtual_file_system).await.unwrap();

    let mut devices_directory =
        virtual_file_system::Directory::open(virtual_file_system, task, Path::DEVICES)
            .await
            .unwrap();
    let mut device_entries = Vec::new();
    while let Some(entry) = devices_directory.read().await.unwrap() {
        device_entries.push((entry.name, entry.kind));
    }
    devices_directory.close(virtual_file_system).await.unwrap();
    assert_eq!(
        device_entries,
        vec![
            (String::from("."), file_system::Kind::Directory),
            (String::from(".."), file_system::Kind::Directory),
            (String::from("uart"), file_system::Kind::Directory),
            (String::from("serial"), file_system::Kind::CharacterDevice),
        ]
    );

    let mut uart_directory =
        virtual_file_system::Directory::open(virtual_file_system, task, Path::UART_DEVICES)
            .await
            .unwrap();
    let mut uart_entries = Vec::new();
    while let Some(entry) = uart_directory.read().await.unwrap() {
        uart_entries.push((entry.name, entry.kind));
    }
    uart_directory.close(virtual_file_system).await.unwrap();
    assert_eq!(
        uart_entries,
        vec![
            (String::from("."), file_system::Kind::Directory),
            (String::from(".."), file_system::Kind::Directory),
            (String::from("uart0"), file_system::Kind::CharacterDevice),
        ]
    );

    assert_eq!(
        counting_device.write_count(),
        writes_after_mount,
        "LittleFS block device must receive zero writes after mounting memory_file_system"
    );
}

/// Character device keeping only terminal attributes, like a serial console.
struct AttributesTerminalDevice {
    attributes: synchronization::blocking_mutex::Mutex<
        synchronization::blocking_mutex::raw::CriticalSectionRawMutex,
        core::cell::Cell<file_system::character_device::TerminalAttributes>,
    >,
}

impl file_system::DirectBaseOperations for AttributesTerminalDevice {
    fn read(
        &self,
        _task_context: &mut core::task::Context<'_>,
        _buffer: &mut [u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        core::task::Poll::Ready(Ok(0))
    }

    fn write(
        &self,
        _task_context: &mut core::task::Context<'_>,
        buffer: &[u8],
        _absolute_position: file_system::Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        core::task::Poll::Ready(Ok(buffer.len()))
    }

    fn control(
        &self,
        _task_context: &mut core::task::Context<'_>,
        command: file_system::ControlCommandIdentifier,
        input: &shared::AnyByLayout,
        output: &mut shared::AnyByLayout,
    ) -> core::task::Poll<file_system::Result<()>> {
        use file_system::ControlCommand;
        use file_system::character_device::{GET_TERMINAL_ATTRIBUTES, SET_TERMINAL_ATTRIBUTES};

        file_system::ready(|| match command {
            GET_TERMINAL_ATTRIBUTES::IDENTIFIER => {
                *GET_TERMINAL_ATTRIBUTES::cast_output(output)? =
                    self.attributes.lock(|attributes| attributes.get());
                Ok(())
            }
            SET_TERMINAL_ATTRIBUTES::IDENTIFIER => {
                let requested = *SET_TERMINAL_ATTRIBUTES::cast_input(input)?;
                if !requested.has_only_known_flags() {
                    return Err(IoErrorKind::InvalidArgument);
                }
                self.attributes.lock(|attributes| attributes.set(requested));
                Ok(())
            }
            _ => Err(IoErrorKind::Unsupported),
        })
    }
}

impl file_system::MountOperations for AttributesTerminalDevice {}
impl file_system::DirectCharacterDevice for AttributesTerminalDevice {}

#[cfg(target_os = "linux")]
#[test]
async fn terminal_attributes_change_through_a_read_only_handle() {
    use file_system::character_device::{
        GET_TERMINAL_ATTRIBUTES, SET_TERMINAL_ATTRIBUTES, TerminalAttributes, TerminalFlags,
    };

    let (task, virtual_file_system) = initialize().await;
    virtual_file_system
        .mount_character_device(
            task,
            Path::from_str("/terminal"),
            AttributesTerminalDevice {
                attributes: synchronization::blocking_mutex::Mutex::new(core::cell::Cell::new(
                    TerminalAttributes::INTERACTIVE,
                )),
            },
        )
        .await
        .unwrap();

    // Programs change the settings of their standard input, opened read-only.
    let mut input = File::open(
        virtual_file_system,
        task,
        "/terminal",
        AccessFlags::Read.into(),
    )
    .await
    .unwrap();

    let saved = input.control(GET_TERMINAL_ATTRIBUTES, &()).await.unwrap();
    assert_eq!(saved, TerminalAttributes::INTERACTIVE);

    input
        .control(SET_TERMINAL_ATTRIBUTES, &saved.with_echo(false))
        .await
        .unwrap();
    let changed = input.control(GET_TERMINAL_ATTRIBUTES, &()).await.unwrap();
    assert!(!changed.is_echo_enabled());
    assert!(changed.is_canonical());

    let unknown = TerminalAttributes::new(TerminalFlags::from_bits_retain(1 << 31));
    assert!(
        input
            .control(SET_TERMINAL_ATTRIBUTES, &unknown)
            .await
            .is_err()
    );

    input
        .control(SET_TERMINAL_ATTRIBUTES, &saved)
        .await
        .unwrap();
    assert_eq!(
        input.control(GET_TERMINAL_ATTRIBUTES, &()).await.unwrap(),
        TerminalAttributes::INTERACTIVE
    );
    input.close(virtual_file_system).await.unwrap();
}

/// The VFS used to flatten every wrapped file system error to `Other` at the
/// embedded-io boundary. One kind now travels from the producer to the caller.
#[cfg(target_os = "linux")]
#[test]
async fn vfs_keeps_not_found_and_permission_denied_through_embedded_io() {
    use embedded_io_async::{Error as _, ErrorKind};
    use file_system::Permissions;
    let (task, vfs) = initialize().await;
    let missing = File::open(vfs, task, "/missing", AccessFlags::Read.into())
        .await
        .err()
        .unwrap();
    assert_eq!(missing, IoErrorKind::NotFound);
    assert_eq!(missing.kind(), ErrorKind::NotFound);

    let user = unprivileged_user(vfs).await;
    File::write_to_path(vfs, task, "/private", b"data")
        .await
        .unwrap();
    vfs.set_permissions(task, "/private", Permissions::from_octal(0o600).unwrap())
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    let denied = File::open(vfs, task, "/private", AccessFlags::Read.into())
        .await
        .err()
        .unwrap();
    assert_eq!(denied, IoErrorKind::PermissionDenied);
    assert_eq!(denied.kind(), ErrorKind::PermissionDenied);
}

/// Readiness (`WouldBlock`) and administrative busy (`ResourceBusy`) have
/// different recovery paths and project to different errno values.
#[cfg(target_os = "linux")]
#[test]
async fn readiness_and_administrative_busy_stay_distinct() {
    use file_system::io_error::PosixErrno;
    let (task, vfs) = initialize().await;
    let (mut reader, _writer) = File::create_unnamed_pipe(vfs, 1, StateFlags::NonBlocking)
        .await
        .unwrap();
    let mut byte = [0];
    let not_ready = reader.read(&mut byte).await.err().unwrap();
    assert_eq!(not_ready, IoErrorKind::WouldBlock);
    assert_eq!(not_ready.posix_errno(), PosixErrno::EAGAIN);

    vfs.create_directory(task, &"/ancestor").await.unwrap();
    mount_test_filesystem(vfs, task, "/ancestor/mount").await;
    let in_use = vfs.remove(task, "/ancestor/mount").await.unwrap_err();
    assert_eq!(in_use, IoErrorKind::ResourceBusy);
    assert_eq!(in_use.posix_errno(), PosixErrno::EBUSY);
}

/// Following too many links is a limit, not corrupt link data.
#[cfg(target_os = "linux")]
#[test]
async fn link_limit_and_invalid_link_target_are_different_failures() {
    let (task, vfs) = initialize().await;
    vfs.create_symlink(task, Path::from_str("/b"), Path::from_str("/a"))
        .await
        .unwrap();
    vfs.create_symlink(task, Path::from_str("/a"), Path::from_str("/b"))
        .await
        .unwrap();
    assert_eq!(
        File::open(vfs, task, "/a", AccessFlags::Read.into())
            .await
            .err(),
        Some(IoErrorKind::SymlinkLoop)
    );
}

/// Caller input errors are reported as such; `CorruptData` is only for a
/// stored link target that cannot be used.
#[cfg(target_os = "linux")]
#[test]
async fn symlink_input_errors_describe_the_argument() {
    use file_system::io_error::PosixErrno;
    let (task, vfs) = initialize().await;

    // POSIX `symlink("", link)` fails with ENOENT.
    let empty = vfs
        .create_symlink(task, Path::from_str(""), Path::from_str("/empty"))
        .await
        .unwrap_err();
    assert_eq!(empty, IoErrorKind::NotFound);
    assert_eq!(empty.posix_errno(), PosixErrno::ENOENT);

    let long_target = "a".repeat(4097);
    let too_long = vfs
        .create_symlink(task, Path::new(&long_target), Path::from_str("/long"))
        .await
        .unwrap_err();
    assert_eq!(too_long, IoErrorKind::NameTooLong);
    assert_eq!(too_long.posix_errno(), PosixErrno::ENAMETOOLONG);

    for (target, link) in [
        ("/bad target", "/invalid-target"),
        ("/target", "relative-link"),
        ("/target", "/"),
        ("/target", "/trailing/"),
    ] {
        let error = vfs
            .create_symlink(task, Path::new(target), Path::new(link))
            .await
            .unwrap_err();
        assert_eq!(error, IoErrorKind::InvalidPath, "{target} -> {link}");
        assert_eq!(error.posix_errno(), PosixErrno::EINVAL);
    }
}

/// Reading a link that is not a link is POSIX `readlink` EINVAL.
#[cfg(target_os = "linux")]
#[test]
async fn read_symlink_on_a_regular_file_is_invalid_argument() {
    use file_system::io_error::PosixErrno;
    let (task, vfs) = initialize().await;
    File::open(
        vfs,
        task,
        "/regular",
        Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    let mut buffer = [0; 16];
    let error = vfs
        .read_symlink(task, Path::from_str("/regular"), &mut buffer)
        .await
        .unwrap_err();
    assert_eq!(error, IoErrorKind::InvalidArgument);
    assert_eq!(error.posix_errno(), PosixErrno::EINVAL);
}

#[cfg(target_os = "linux")]
mod symlink_confinement_regression_tests {
    use super::*;
    use core::{
        future::Future,
        pin::pin,
        task::{Context as TaskContext, Poll, Waker},
    };
    use file_system::{
        AttributeFlags, AttributeOperations, Attributes, BaseOperations, DirectoryOperations,
        Entry, FileOperations, FileSystemOperations, Inode, Kind, MountOperations, Permissions,
        Size, Time,
    };
    use std::{
        collections::BTreeMap,
        sync::{Mutex, MutexGuard},
    };
    use users::{GroupIdentifier, UserIdentifier};

    #[derive(Clone)]
    enum SimulatedNodeKind {
        Directory { entries: BTreeMap<String, Inode> },
        File { contents: Vec<u8> },
        SymbolicLink { target: String },
    }

    #[derive(Clone)]
    struct SimulatedNode {
        inode: Inode,
        kind: SimulatedNodeKind,
        user: UserIdentifier,
        group: GroupIdentifier,
        permissions: Permissions,
        creation: Time,
        modification: Time,
        access: Time,
        status: Time,
    }

    #[derive(Clone)]
    enum EntrySwapAction {
        RenameDirectoryAndReplaceWithSymlink {
            parent_inode: Inode,
            old_name: String,
            new_name: String,
            symlink_target: String,
        },
        ReplaceEntryWithSymlink {
            parent_inode: Inode,
            name: String,
            symlink_target: String,
        },
    }

    struct ScheduledHook {
        trigger_name: String,
        action: EntrySwapAction,
        after_attribute_check: bool,
    }

    struct SimulatedState {
        next_inode: Inode,
        nodes: BTreeMap<Inode, SimulatedNode>,
        hook: Option<ScheduledHook>,
    }

    #[derive(Clone, Copy)]
    struct HandleState {
        inode: Inode,
        position: Size,
    }

    #[derive(Clone, Copy)]
    struct SimulatedDirectoryEntryFileSystem {
        state: &'static Mutex<SimulatedState>,
        confined_namespace: bool,
    }

    impl SimulatedDirectoryEntryFileSystem {
        fn new() -> Self {
            Self::with_confinement(true)
        }

        fn new_unconfined() -> Self {
            Self::with_confinement(false)
        }

        fn with_confinement(confined_namespace: bool) -> Self {
            let mut nodes = BTreeMap::new();
            nodes.insert(
                1,
                SimulatedNode {
                    inode: 1,
                    kind: SimulatedNodeKind::Directory {
                        entries: BTreeMap::new(),
                    },
                    user: UserIdentifier::ROOT,
                    group: GroupIdentifier::ROOT,
                    permissions: Permissions::DIRECTORY_DEFAULT,
                    creation: Time::new(1),
                    modification: Time::new(1),
                    access: Time::new(1),
                    status: Time::new(1),
                },
            );
            Self {
                state: Box::leak(Box::new(Mutex::new(SimulatedState {
                    next_inode: 2,
                    nodes,
                    hook: None,
                }))),
                confined_namespace,
            }
        }

        fn state(&self) -> MutexGuard<'_, SimulatedState> {
            self.state.lock().unwrap()
        }

        fn schedule_hook_on_entry_check(
            &self,
            trigger_name: &str,
            action: EntrySwapAction,
            after_attribute_check: bool,
        ) {
            self.state().hook = Some(ScheduledHook {
                trigger_name: String::from(trigger_name),
                action,
                after_attribute_check,
            });
        }

        fn insert_node(
            state: &mut SimulatedState,
            parent_inode: Inode,
            name: &str,
            kind: SimulatedNodeKind,
            permissions: Permissions,
        ) -> file_system::Result<Inode> {
            if let Some(SimulatedNode {
                kind: SimulatedNodeKind::Directory { entries },
                ..
            }) = state.nodes.get(&parent_inode)
                && entries.contains_key(name)
            {
                return Err(IoErrorKind::AlreadyExists);
            }
            let inode = state.next_inode;
            state.next_inode += 1;
            state.nodes.insert(
                inode,
                SimulatedNode {
                    inode,
                    kind,
                    user: UserIdentifier::ROOT,
                    group: GroupIdentifier::ROOT,
                    permissions,
                    creation: Time::new(1),
                    modification: Time::new(1),
                    access: Time::new(1),
                    status: Time::new(1),
                },
            );
            if let Some(SimulatedNode {
                kind: SimulatedNodeKind::Directory { entries },
                ..
            }) = state.nodes.get_mut(&parent_inode)
            {
                entries.insert(String::from(name), inode);
            } else {
                return Err(IoErrorKind::NotADirectory);
            }
            Ok(inode)
        }

        fn populate_directory(
            &self,
            parent_inode: Inode,
            name: &str,
        ) -> file_system::Result<Inode> {
            let mut state = self.state();
            Self::insert_node(
                &mut state,
                parent_inode,
                name,
                SimulatedNodeKind::Directory {
                    entries: BTreeMap::new(),
                },
                Permissions::DIRECTORY_DEFAULT,
            )
        }

        fn populate_file(
            &self,
            parent_inode: Inode,
            name: &str,
            contents: &[u8],
        ) -> file_system::Result<Inode> {
            let mut state = self.state();
            Self::insert_node(
                &mut state,
                parent_inode,
                name,
                SimulatedNodeKind::File {
                    contents: contents.to_vec(),
                },
                Permissions::FILE_DEFAULT,
            )
        }

        fn populate_symlink(
            &self,
            parent_inode: Inode,
            name: &str,
            target: &str,
        ) -> file_system::Result<Inode> {
            let mut state = self.state();
            Self::insert_node(
                &mut state,
                parent_inode,
                name,
                SimulatedNodeKind::SymbolicLink {
                    target: String::from(target),
                },
                Permissions::ALL_FULL,
            )
        }

        fn apply_action(state: &mut SimulatedState, action: EntrySwapAction) {
            match action {
                EntrySwapAction::RenameDirectoryAndReplaceWithSymlink {
                    parent_inode,
                    old_name,
                    new_name,
                    symlink_target,
                } => {
                    let moved_inode = if let Some(SimulatedNode {
                        kind: SimulatedNodeKind::Directory { entries },
                        ..
                    }) = state.nodes.get_mut(&parent_inode)
                    {
                        entries
                            .remove(&old_name)
                            .expect("directory entry to rename must exist")
                    } else {
                        panic!("expected directory node");
                    };
                    if let Some(SimulatedNode {
                        kind: SimulatedNodeKind::Directory { entries },
                        ..
                    }) = state.nodes.get_mut(&parent_inode)
                    {
                        entries.insert(new_name, moved_inode);
                    }
                    Self::insert_node(
                        state,
                        parent_inode,
                        &old_name,
                        SimulatedNodeKind::SymbolicLink {
                            target: symlink_target,
                        },
                        Permissions::ALL_FULL,
                    )
                    .unwrap();
                }
                EntrySwapAction::ReplaceEntryWithSymlink {
                    parent_inode,
                    name,
                    symlink_target,
                } => {
                    if let Some(SimulatedNode {
                        kind: SimulatedNodeKind::Directory { entries },
                        ..
                    }) = state.nodes.get_mut(&parent_inode)
                    {
                        entries.remove(&name);
                    }
                    Self::insert_node(
                        state,
                        parent_inode,
                        &name,
                        SimulatedNodeKind::SymbolicLink {
                            target: symlink_target,
                        },
                        Permissions::ALL_FULL,
                    )
                    .unwrap();
                }
            }
        }

        fn context_inode(context: &Context) -> file_system::Result<Inode> {
            context
                .get_private_data_of_type::<HandleState>()
                .map(|handle| handle.inode)
                .ok_or(IoErrorKind::BadDescriptor)
        }

        fn lookup_child_inode(
            state: &SimulatedState,
            parent_inode: Inode,
            name: &str,
        ) -> file_system::Result<Inode> {
            let parent = state
                .nodes
                .get(&parent_inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            let SimulatedNodeKind::Directory { entries } = &parent.kind else {
                return Err(IoErrorKind::NotADirectory);
            };
            entries.get(name).copied().ok_or(IoErrorKind::NotFound)
        }

        fn read_node_attributes(node: &SimulatedNode, attributes: &mut Attributes) {
            let request = attributes.get_mask();
            let mut output = Attributes::new();
            let mut populated = AttributeFlags::empty();
            if request.contains(AttributeFlags::Inode) {
                output = output.set_inode(node.inode);
                populated.insert(AttributeFlags::Inode);
            }
            if request.contains(AttributeFlags::Kind) {
                let kind = match node.kind {
                    SimulatedNodeKind::Directory { .. } => Kind::Directory,
                    SimulatedNodeKind::File { .. } => Kind::File,
                    SimulatedNodeKind::SymbolicLink { .. } => Kind::SymbolicLink,
                };
                output = output.set_kind(kind);
                populated.insert(AttributeFlags::Kind);
            }
            if request.contains(AttributeFlags::Size) {
                let size = match &node.kind {
                    SimulatedNodeKind::Directory { entries } => entries.len() as Size,
                    SimulatedNodeKind::File { contents } => contents.len() as Size,
                    SimulatedNodeKind::SymbolicLink { target } => target.len() as Size,
                };
                output = output.set_size(size);
                populated.insert(AttributeFlags::Size);
            }
            if request.contains(AttributeFlags::User) {
                output = output.set_user(node.user);
                populated.insert(AttributeFlags::User);
            }
            if request.contains(AttributeFlags::Group) {
                output = output.set_group(node.group);
                populated.insert(AttributeFlags::Group);
            }
            if request.contains(AttributeFlags::Permissions) {
                output = output.set_permissions(node.permissions);
                populated.insert(AttributeFlags::Permissions);
            }
            if request.contains(AttributeFlags::CreationTime) {
                output = output.set_creation(node.creation);
                populated.insert(AttributeFlags::CreationTime);
            }
            if request.contains(AttributeFlags::ModificationTime) {
                output = output.set_modification(node.modification);
                populated.insert(AttributeFlags::ModificationTime);
            }
            if request.contains(AttributeFlags::AccessTime) {
                output = output.set_access(node.access);
                populated.insert(AttributeFlags::AccessTime);
            }
            if request.contains(AttributeFlags::StatusTime) {
                output = output.set_status(node.status);
                populated.insert(AttributeFlags::StatusTime);
            }
            *attributes = output.set_mask(populated);
        }

        fn write_node_attributes(node: &mut SimulatedNode, attributes: &Attributes) {
            if let Some(&user) = attributes.get_user() {
                node.user = user;
            }
            if let Some(&group) = attributes.get_group() {
                node.group = group;
            }
            if let Some(&permissions) = attributes.get_permissions() {
                node.permissions = permissions;
            }
            if let Some(&creation) = attributes.get_creation() {
                node.creation = creation;
            }
            if let Some(&modification) = attributes.get_modification() {
                node.modification = modification;
            }
            if let Some(&access) = attributes.get_access() {
                node.access = access;
            }
            if let Some(&status) = attributes.get_status() {
                node.status = status;
            }
        }
    }

    impl MountOperations for SimulatedDirectoryEntryFileSystem {}

    impl BaseOperations for SimulatedDirectoryEntryFileSystem {
        fn read(
            &self,
            context: &mut Context,
            _context_waker: &mut TaskContext<'_>,
            buffer: &mut [u8],
            position: Size,
        ) -> Poll<file_system::Result<usize>> {
            Poll::Ready((|| {
                let inode = Self::context_inode(context)?;
                let state = self.state();
                let node = state.nodes.get(&inode).ok_or(IoErrorKind::StaleHandle)?;
                let SimulatedNodeKind::File { contents } = &node.kind else {
                    return Err(IoErrorKind::IsADirectory);
                };
                let offset = usize::try_from(position).map_err(|_| IoErrorKind::InvalidArgument)?;
                if offset >= contents.len() {
                    return Ok(0);
                }
                let available = contents.len() - offset;
                let copied = available.min(buffer.len());
                buffer[..copied].copy_from_slice(&contents[offset..offset + copied]);
                Ok(copied)
            })())
        }

        fn write(
            &self,
            context: &mut Context,
            _context_waker: &mut TaskContext<'_>,
            buffer: &[u8],
            position: Size,
        ) -> Poll<file_system::Result<usize>> {
            Poll::Ready((|| {
                let inode = Self::context_inode(context)?;
                let mut state = self.state();
                let node = state
                    .nodes
                    .get_mut(&inode)
                    .ok_or(IoErrorKind::StaleHandle)?;
                let SimulatedNodeKind::File { contents } = &mut node.kind else {
                    return Err(IoErrorKind::IsADirectory);
                };
                let offset = usize::try_from(position).map_err(|_| IoErrorKind::InvalidArgument)?;
                let end = offset
                    .checked_add(buffer.len())
                    .ok_or(IoErrorKind::InvalidArgument)?;
                if contents.len() < end {
                    contents.resize(end, 0);
                }
                contents[offset..end].copy_from_slice(buffer);
                Ok(buffer.len())
            })())
        }

        fn set_position(
            &self,
            context: &mut Context,
            _position: Size,
            from: &Position,
        ) -> file_system::Result<Size> {
            let inode = Self::context_inode(context)?;
            let state = self.state();
            let node = state.nodes.get(&inode).ok_or(IoErrorKind::StaleHandle)?;
            let size = match &node.kind {
                SimulatedNodeKind::File { contents } => contents.len() as Size,
                SimulatedNodeKind::Directory { entries } => entries.len() as Size,
                SimulatedNodeKind::SymbolicLink { target } => target.len() as Size,
            };
            let handle = context
                .get_private_data_mutable_of_type::<HandleState>()
                .ok_or(IoErrorKind::BadDescriptor)?;
            let target = match from {
                Position::Start(offset) => *offset,
                Position::Current(delta) => handle
                    .position
                    .checked_add_signed(*delta)
                    .ok_or(IoErrorKind::InvalidArgument)?,
                Position::End(delta) => size
                    .checked_add_signed(*delta)
                    .ok_or(IoErrorKind::InvalidArgument)?,
            };
            handle.position = target;
            Ok(target)
        }

        fn clone_context(&self, context: &Context) -> file_system::Result<Context> {
            let handle = context
                .get_private_data_of_type::<HandleState>()
                .ok_or(IoErrorKind::BadDescriptor)?;
            Ok(Context::new(Some(*handle)))
        }

        fn close(&self, context: &mut Context) -> file_system::Result<()> {
            let _ = context.take_private_data_of_type::<HandleState>();
            Ok(())
        }
    }

    impl FileOperations for SimulatedDirectoryEntryFileSystem {}

    impl AttributeOperations for SimulatedDirectoryEntryFileSystem {
        fn get_attributes(
            &self,
            context: &mut Context,
            attributes: &mut Attributes,
        ) -> file_system::Result<()> {
            let inode = Self::context_inode(context)?;
            let state = self.state();
            let node = state.nodes.get(&inode).ok_or(IoErrorKind::StaleHandle)?;
            Self::read_node_attributes(node, attributes);
            Ok(())
        }

        fn set_attributes(
            &self,
            context: &mut Context,
            attributes: &Attributes,
        ) -> file_system::Result<()> {
            let inode = Self::context_inode(context)?;
            let mut state = self.state();
            let node = state
                .nodes
                .get_mut(&inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            Self::write_node_attributes(node, attributes);
            Ok(())
        }
    }

    impl DirectoryOperations for SimulatedDirectoryEntryFileSystem {
        fn read(&self, context: &mut Context) -> file_system::Result<Option<Entry>> {
            let handle = context
                .get_private_data_mutable_of_type::<HandleState>()
                .ok_or(IoErrorKind::BadDescriptor)?;
            let position =
                usize::try_from(handle.position).map_err(|_| IoErrorKind::InvalidArgument)?;
            let state = self.state();
            let directory = state
                .nodes
                .get(&handle.inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            let SimulatedNodeKind::Directory { entries } = &directory.kind else {
                return Err(IoErrorKind::NotADirectory);
            };
            let Some((name, child_inode)) = entries.iter().nth(position) else {
                return Ok(None);
            };
            let child = state
                .nodes
                .get(child_inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            let kind = match child.kind {
                SimulatedNodeKind::Directory { .. } => Kind::Directory,
                SimulatedNodeKind::File { .. } => Kind::File,
                SimulatedNodeKind::SymbolicLink { .. } => Kind::SymbolicLink,
            };
            let size = match &child.kind {
                SimulatedNodeKind::Directory { entries } => entries.len() as Size,
                SimulatedNodeKind::File { contents } => contents.len() as Size,
                SimulatedNodeKind::SymbolicLink { target } => target.len() as Size,
            };
            handle.position += 1;
            Ok(Some(Entry::new(
                child.inode,
                name.clone(),
                kind,
                size,
                child.modification,
            )))
        }

        fn set_position(&self, context: &mut Context, position: Size) -> file_system::Result<()> {
            let handle = context
                .get_private_data_mutable_of_type::<HandleState>()
                .ok_or(IoErrorKind::BadDescriptor)?;
            handle.position = position;
            Ok(())
        }

        fn get_position(&self, context: &mut Context) -> file_system::Result<Size> {
            let handle = context
                .get_private_data_of_type::<HandleState>()
                .ok_or(IoErrorKind::BadDescriptor)?;
            Ok(handle.position)
        }

        fn rewind(&self, context: &mut Context) -> file_system::Result<()> {
            let handle = context
                .get_private_data_mutable_of_type::<HandleState>()
                .ok_or(IoErrorKind::BadDescriptor)?;
            handle.position = 0;
            Ok(())
        }

        fn close(&self, context: &mut Context) -> file_system::Result<()> {
            let _ = context.take_private_data_of_type::<HandleState>();
            Ok(())
        }
    }

    impl FileSystemOperations for SimulatedDirectoryEntryFileSystem {
        fn supports_confined_namespace(&self) -> bool {
            self.confined_namespace
        }

        fn supports_directory_entry_operations(&self) -> bool {
            true
        }

        fn lookup_directory(&self, context: &mut Context, path: &Path) -> file_system::Result<()> {
            assert!(
                path.is_root(),
                "directory-entry backend must never re-resolve a non-root path"
            );
            *context = Context::new(Some(HandleState {
                inode: 1,
                position: 0,
            }));
            Ok(())
        }

        fn lookup_directory_entry(
            &self,
            parent: &Context,
            name: &str,
            context: &mut Context,
        ) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let state = self.state();
            let child_inode = Self::lookup_child_inode(&state, parent_inode, name)?;
            let child = state
                .nodes
                .get(&child_inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            match child.kind {
                SimulatedNodeKind::Directory { .. } => {
                    *context = Context::new(Some(HandleState {
                        inode: child_inode,
                        position: 0,
                    }));
                    Ok(())
                }
                SimulatedNodeKind::SymbolicLink { .. } => Err(IoErrorKind::SymlinkLoop),
                SimulatedNodeKind::File { .. } => Err(IoErrorKind::NotADirectory),
            }
        }

        fn lookup_file(
            &self,
            _context: &mut Context,
            _path: &Path,
            _flags: Flags,
        ) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based lookup_file");
        }

        fn lookup_file_entry(
            &self,
            parent: &Context,
            name: &str,
            context: &mut Context,
            flags: Flags,
        ) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            let child_inode = Self::lookup_child_inode(&state, parent_inode, name)?;
            let is_directory = matches!(
                state.nodes.get(&child_inode),
                Some(SimulatedNode {
                    kind: SimulatedNodeKind::Directory { .. },
                    ..
                })
            );
            let is_symlink = matches!(
                state.nodes.get(&child_inode),
                Some(SimulatedNode {
                    kind: SimulatedNodeKind::SymbolicLink { .. },
                    ..
                })
            );
            if is_directory {
                return Err(IoErrorKind::IsADirectory);
            }
            if is_symlink {
                return Err(IoErrorKind::SymlinkLoop);
            }
            if flags.get_create().contains(CreateFlags::Truncate)
                && let Some(SimulatedNode {
                    kind: SimulatedNodeKind::File { contents },
                    ..
                }) = state.nodes.get_mut(&child_inode)
            {
                contents.clear();
            }
            *context = Context::new(Some(HandleState {
                inode: child_inode,
                position: 0,
            }));
            Ok(())
        }

        fn create_directory(&self, _path: &Path) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based create_directory");
        }

        fn create_directory_entry(&self, parent: &Context, name: &str) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            Self::insert_node(
                &mut state,
                parent_inode,
                name,
                SimulatedNodeKind::Directory {
                    entries: BTreeMap::new(),
                },
                Permissions::DIRECTORY_DEFAULT,
            )?;
            Ok(())
        }

        fn create_file(&self, _path: &Path) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based create_file");
        }

        fn create_file_entry(&self, parent: &Context, name: &str) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            Self::insert_node(
                &mut state,
                parent_inode,
                name,
                SimulatedNodeKind::File {
                    contents: Vec::new(),
                },
                Permissions::FILE_DEFAULT,
            )?;
            Ok(())
        }

        fn remove(&self, _path: &Path) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based remove");
        }

        fn remove_entry(&self, parent: &Context, name: &str) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            let child_inode = Self::lookup_child_inode(&state, parent_inode, name)?;
            if let Some(SimulatedNode {
                kind: SimulatedNodeKind::Directory { entries },
                ..
            }) = state.nodes.get(&child_inode)
                && !entries.is_empty()
            {
                return Err(IoErrorKind::DirectoryNotEmpty);
            }
            if let Some(SimulatedNode {
                kind: SimulatedNodeKind::Directory { entries },
                ..
            }) = state.nodes.get_mut(&parent_inode)
            {
                entries.remove(name);
            }
            state.nodes.remove(&child_inode);
            Ok(())
        }

        fn rename(&self, _old_path: &Path, _new_path: &Path) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based rename");
        }

        fn rename_entry(
            &self,
            old_parent: &Context,
            old_name: &str,
            new_parent: &Context,
            new_name: &str,
        ) -> file_system::Result<()> {
            let old_parent_inode = Self::context_inode(old_parent)?;
            let new_parent_inode = Self::context_inode(new_parent)?;
            let mut state = self.state();
            let moved_inode = Self::lookup_child_inode(&state, old_parent_inode, old_name)?;
            if let Some(SimulatedNode {
                kind: SimulatedNodeKind::Directory { entries },
                ..
            }) = state.nodes.get_mut(&old_parent_inode)
            {
                entries.remove(old_name);
            }
            if let Some(SimulatedNode {
                kind: SimulatedNodeKind::Directory { entries },
                ..
            }) = state.nodes.get_mut(&new_parent_inode)
            {
                entries.insert(String::from(new_name), moved_inode);
            }
            Ok(())
        }

        fn create_symlink(&self, _target: &Path, _link: &Path) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based create_symlink");
        }

        fn create_symlink_entry(
            &self,
            target: &Path,
            parent: &Context,
            name: &str,
        ) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            Self::insert_node(
                &mut state,
                parent_inode,
                name,
                SimulatedNodeKind::SymbolicLink {
                    target: String::from(target.as_str()),
                },
                Permissions::ALL_FULL,
            )?;
            Ok(())
        }

        fn read_symlink(&self, _link: &Path, _buffer: &mut [u8]) -> file_system::Result<usize> {
            panic!("directory-entry backend must not receive path-based read_symlink");
        }

        fn read_symlink_entry(
            &self,
            parent: &Context,
            name: &str,
            buffer: &mut [u8],
        ) -> file_system::Result<usize> {
            let parent_inode = Self::context_inode(parent)?;
            let state = self.state();
            let child_inode = Self::lookup_child_inode(&state, parent_inode, name)?;
            let child = state
                .nodes
                .get(&child_inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            let SimulatedNodeKind::SymbolicLink { target } = &child.kind else {
                return Err(IoErrorKind::InvalidArgument);
            };
            let length = target.len().min(buffer.len());
            buffer[..length].copy_from_slice(&target.as_bytes()[..length]);
            Ok(length)
        }

        fn get_attributes(
            &self,
            _path: &Path,
            _attributes: &mut Attributes,
        ) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based get_attributes");
        }

        fn get_entry_attributes(
            &self,
            parent: &Context,
            name: &str,
            attributes: &mut Attributes,
        ) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            let pending_hook = state
                .hook
                .as_ref()
                .is_some_and(|hook| hook.trigger_name == name)
                .then(|| state.hook.take())
                .flatten();
            if let Some(hook) = pending_hook.as_ref()
                && !hook.after_attribute_check
            {
                Self::apply_action(&mut state, hook.action.clone());
            }
            let child_inode = Self::lookup_child_inode(&state, parent_inode, name)?;
            let child = state
                .nodes
                .get(&child_inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            Self::read_node_attributes(child, attributes);
            if let Some(hook) = pending_hook
                && hook.after_attribute_check
            {
                Self::apply_action(&mut state, hook.action);
            }
            Ok(())
        }

        fn set_attributes(
            &self,
            _path: &Path,
            _attributes: &Attributes,
        ) -> file_system::Result<()> {
            panic!("directory-entry backend must not receive path-based set_attributes");
        }

        fn set_entry_attributes(
            &self,
            parent: &Context,
            name: &str,
            attributes: &Attributes,
        ) -> file_system::Result<()> {
            let parent_inode = Self::context_inode(parent)?;
            let mut state = self.state();
            let child_inode = Self::lookup_child_inode(&state, parent_inode, name)?;
            let child = state
                .nodes
                .get_mut(&child_inode)
                .ok_or(IoErrorKind::StaleHandle)?;
            Self::write_node_attributes(child, attributes);
            Ok(())
        }

        fn writable_attributes(&self) -> AttributeFlags {
            AttributeFlags::User
                | AttributeFlags::Group
                | AttributeFlags::Permissions
                | AttributeFlags::CreationTime
                | AttributeFlags::ModificationTime
                | AttributeFlags::AccessTime
                | AttributeFlags::StatusTime
        }

        fn persistent_attributes(&self) -> AttributeFlags {
            self.writable_attributes()
        }
    }

    fn new_little_file_system() -> little_fs::FileSystem {
        let device = MemoryDevice::<512>::new_static(1024 * 512);
        let options = little_fs::Options::new(256);
        little_fs::FileSystem::format(device, options).unwrap();
        little_fs::FileSystem::new(device, options).unwrap()
    }

    fn poll_uncontended<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let waker = Waker::noop();
        let mut task_context = TaskContext::from_waker(waker);
        match future.as_mut().poll(&mut task_context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("uncontended VFS test future should resolve without yielding"),
        }
    }

    async fn write_new_file(
        virtual_file_system: &VirtualFileSystem,
        task: TaskIdentifier,
        path: &str,
        bytes: &[u8],
    ) {
        let mut file = File::open(
            virtual_file_system,
            task,
            path,
            Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
        )
        .await
        .unwrap();
        file.write(bytes).await.unwrap();
    }

    async fn read_to_vec(file: &mut File<'_>) -> Vec<u8> {
        let mut buffer = [0u8; 64];
        let mut out = Vec::new();
        loop {
            let read = file.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buffer[..read]);
        }
        out
    }

    #[cfg(target_os = "linux")]
    #[task::test]
    async fn resolve_directory_root_resolves_once_and_rejects_invalid_roots() {
        let (task, virtual_file_system) = initialize().await;

        virtual_file_system
            .create_directory(task, &"/real_preopen")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/real_preopen/nested")
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/real_preopen/nested/data.txt",
            b"preopen-ok",
        )
        .await;
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("/real_preopen"),
                Path::from_str("/preopen_link"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("preopen_link/nested"),
                Path::from_str("/nested_link"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("/does_not_exist"),
                Path::from_str("/dangling_root"),
            )
            .await
            .unwrap();

        let resolved_root = virtual_file_system
            .resolve_directory_root(task, &"/nested_link")
            .await
            .unwrap();
        assert_eq!(resolved_root.as_str(), "/real_preopen/nested");

        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"data.txt",
                    Flags::from(AccessFlags::Read),
                    task,
                    &resolved_root,
                )
                .await
                .map(|_| ()),
            Ok(())
        );
        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"data.txt",
                    Flags::from(AccessFlags::Read),
                    task,
                    Path::from_str("/nested_link"),
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
        assert_eq!(
            virtual_file_system
                .resolve_directory_root(task, &"/real_preopen/nested/data.txt")
                .await
                .unwrap_err(),
            IoErrorKind::NotADirectory
        );
        assert_eq!(
            virtual_file_system
                .resolve_directory_root(task, &"/dangling_root")
                .await
                .unwrap_err(),
            IoErrorKind::NotFound
        );

        virtual_file_system
            .create_directory(task, &"/unconfined_mount")
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(
                SimulatedDirectoryEntryFileSystem::new_unconfined(),
                &"/unconfined_mount",
                task,
            )
            .await
            .unwrap();
        assert_eq!(
            virtual_file_system
                .resolve_directory_root(task, &"/unconfined_mount")
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
        assert!(
            virtual_file_system
                .open_directory_beneath(task, &"nested//", Path::from_str("/real_preopen"))
                .await
                .is_ok()
        );
    }

    #[cfg(target_os = "linux")]
    #[task::test]
    async fn confined_chained_links_and_parent_traversal_stay_inside_preopen() {
        let (task, virtual_file_system) = initialize().await;

        write_new_file(virtual_file_system, task, "/outside.txt", b"outside").await;
        virtual_file_system
            .create_directory(task, &"/sandbox")
            .await
            .unwrap();
        write_new_file(virtual_file_system, task, "/sandbox/inside.txt", b"inside").await;
        virtual_file_system
            .create_directory(task, &"/sandbox/a")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/a/b")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/a/c")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/a/c/subdirectory")
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/sandbox/a/c/subdirectory/target.txt",
            b"chained-ok",
        )
        .await;

        virtual_file_system
            .create_directory(task, &"/sandbox/directory_1")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/directory_2")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/directory_2/nested")
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/sandbox/directory_2/sibling.txt",
            b"parent-after-link-ok",
        )
        .await;

        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("../c/link2"),
                Path::from_str("/sandbox/a/b/link1"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("subdirectory/target.txt"),
                Path::from_str("/sandbox/a/c/link2"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("../directory_2/nested"),
                Path::from_str("/sandbox/directory_1/to_directory_2"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str(".."),
                Path::from_str("/sandbox/directory_1/to_root"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("/sandbox/inside.txt"),
                Path::from_str("/sandbox/absolute_inside"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("/outside.txt"),
                Path::from_str("/sandbox/absolute_outside"),
            )
            .await
            .unwrap();

        let root = virtual_file_system
            .resolve_directory_root(task, &"/sandbox")
            .await
            .unwrap();

        let mut chained = virtual_file_system
            .open_beneath(&"a/b/link1", Flags::from(AccessFlags::Read), task, &root)
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut chained).await, b"chained-ok");

        let mut via_parent = virtual_file_system
            .open_beneath(
                &"directory_1/to_directory_2/../sibling.txt",
                Flags::from(AccessFlags::Read),
                task,
                &root,
            )
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut via_parent).await, b"parent-after-link-ok");

        assert!(
            virtual_file_system
                .open_directory_beneath(task, &"directory_1/to_directory_2/..", &root)
                .await
                .is_ok()
        );
        assert!(
            virtual_file_system
                .open_directory_beneath(task, &"directory_1/to_directory_2/../..", &root)
                .await
                .is_ok()
        );

        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"directory_1/to_root/../outside.txt",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"directory_1/../..",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );

        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"absolute_inside",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"absolute_outside",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );

        let mut unconfined = virtual_file_system
            .open(
                &"/sandbox/absolute_inside",
                Flags::from(AccessFlags::Read),
                task,
            )
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut unconfined).await, b"inside");
    }

    #[cfg(target_os = "linux")]
    #[task::test]
    async fn confined_cross_mount_links_stay_confined_across_submounts() {
        let (task, virtual_file_system) = initialize().await;

        write_new_file(virtual_file_system, task, "/outside.txt", b"outside").await;
        virtual_file_system
            .create_directory(task, &"/outside_mount")
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(new_little_file_system(), &"/outside_mount", task)
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/outside_mount/secret.txt",
            b"secret",
        )
        .await;

        virtual_file_system
            .create_directory(task, &"/sandbox")
            .await
            .unwrap();
        write_new_file(virtual_file_system, task, "/sandbox/inside.txt", b"inside").await;
        virtual_file_system
            .create_directory(task, &"/sandbox/mounted")
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(new_little_file_system(), &"/sandbox/mounted", task)
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/sandbox/mounted/file.txt",
            b"submount-ok",
        )
        .await;
        virtual_file_system
            .create_directory(task, &"/sandbox/mounted/deep")
            .await
            .unwrap();

        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("mounted/file.txt"),
                Path::from_str("/sandbox/to_submount"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("../../inside.txt"),
                Path::from_str("/sandbox/mounted/deep/back"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("../../../outside.txt"),
                Path::from_str("/sandbox/mounted/deep/escape"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("/outside_mount/secret.txt"),
                Path::from_str("/sandbox/to_outside_mount"),
            )
            .await
            .unwrap();

        let root = virtual_file_system
            .resolve_directory_root(task, &"/sandbox")
            .await
            .unwrap();

        let mut into_mount = virtual_file_system
            .open_beneath(&"to_submount", Flags::from(AccessFlags::Read), task, &root)
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut into_mount).await, b"submount-ok");

        let mut back_from_mount = virtual_file_system
            .open_beneath(
                &"mounted/deep/back",
                Flags::from(AccessFlags::Read),
                task,
                &root,
            )
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut back_from_mount).await, b"inside");

        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"mounted/deep/escape",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"to_outside_mount",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
    }

    #[cfg(target_os = "linux")]
    #[task::test]
    async fn confined_dangling_links_and_no_follow_final_component() {
        let (task, virtual_file_system) = initialize().await;

        virtual_file_system
            .create_directory(task, &"/sandbox")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/real_directory")
            .await
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/sandbox/real_directory/subdirectory")
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/sandbox/real_directory/target.txt",
            b"hello",
        )
        .await;
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("real_directory"),
                Path::from_str("/sandbox/directory_link"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("real_directory/target.txt"),
                Path::from_str("/sandbox/file_link"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("missing.txt"),
                Path::from_str("/sandbox/dangling"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("never_created.txt"),
                Path::from_str("/sandbox/dangling_exclusive"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("../outside_created.txt"),
                Path::from_str("/sandbox/dangling_escape"),
            )
            .await
            .unwrap();

        let root = virtual_file_system
            .resolve_directory_root(task, &"/sandbox")
            .await
            .unwrap();

        assert_eq!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"dangling",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                    true,
                )
                .await
                .unwrap_err(),
            IoErrorKind::NotFound
        );
        assert_eq!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"dangling",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                    false,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );
        assert_eq!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"dangling",
                    Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
                    task,
                    &root,
                    false,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );
        assert_eq!(
            virtual_file_system
                .open(
                    &"/sandbox/missing.txt",
                    Flags::from(AccessFlags::Read),
                    task
                )
                .await
                .unwrap_err(),
            IoErrorKind::NotFound
        );

        {
            let mut created = virtual_file_system
                .open_beneath_with_lookup(
                    &"dangling",
                    Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
                    task,
                    &root,
                    true,
                )
                .await
                .unwrap();
            created.write(b"created-through-dangling").await.unwrap();
        }
        let mut read_back = virtual_file_system
            .open(
                &"/sandbox/missing.txt",
                Flags::from(AccessFlags::Read),
                task,
            )
            .await
            .unwrap();
        assert_eq!(
            read_to_vec(&mut read_back).await,
            b"created-through-dangling"
        );

        for follow in [false, true] {
            assert_eq!(
                virtual_file_system
                    .open_beneath_with_lookup(
                        &"dangling_exclusive",
                        Flags::new(
                            AccessFlags::READ_WRITE,
                            Some(CreateFlags::CREATE_EXCLUSIVE),
                            None,
                        ),
                        task,
                        &root,
                        follow,
                    )
                    .await
                    .unwrap_err(),
                IoErrorKind::AlreadyExists
            );
        }
        assert_eq!(
            virtual_file_system
                .open(
                    &"/sandbox/never_created.txt",
                    Flags::from(AccessFlags::Read),
                    task,
                )
                .await
                .unwrap_err(),
            IoErrorKind::NotFound
        );

        assert_eq!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"dangling_escape",
                    Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
                    task,
                    &root,
                    true,
                )
                .await
                .unwrap_err(),
            IoErrorKind::PathEscapesRoot
        );
        assert_eq!(
            virtual_file_system
                .open(
                    &"/outside_created.txt",
                    Flags::from(AccessFlags::Read),
                    task
                )
                .await
                .unwrap_err(),
            IoErrorKind::NotFound
        );

        assert_eq!(
            virtual_file_system
                .open_directory_beneath_with_lookup(task, &"directory_link", &root, false)
                .await
                .err()
                .unwrap(),
            IoErrorKind::SymlinkLoop
        );
        assert!(
            virtual_file_system
                .open_directory_beneath_with_lookup(task, &"directory_link", &root, true)
                .await
                .is_ok()
        );
        assert!(
            virtual_file_system
                .open_directory_beneath_with_lookup(
                    task,
                    &"directory_link/subdirectory",
                    &root,
                    false,
                )
                .await
                .is_ok()
        );
        assert!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"directory_link/target.txt",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                    false,
                )
                .await
                .is_ok()
        );
        assert_eq!(
            virtual_file_system
                .open_directory_with_lookup(task, &"/sandbox/directory_link", false)
                .await
                .err()
                .unwrap(),
            IoErrorKind::SymlinkLoop
        );
        assert_eq!(
            virtual_file_system
                .open_with_lookup(
                    &"/sandbox/file_link",
                    Flags::from(AccessFlags::Read),
                    task,
                    false,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );
    }

    #[cfg(target_os = "linux")]
    #[task::test]
    async fn symlink_loop_bound_at_40_links_and_constant_allocation() {
        let (task, virtual_file_system) = initialize().await;

        // Exercise LittleFS chained links up to the 40-link limit and past it.
        virtual_file_system
            .create_directory(task, &"/little_file_system_chain")
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/little_file_system_chain/end",
            b"at-bound",
        )
        .await;
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("end"),
                Path::from_str("/little_file_system_chain/s01"),
            )
            .await
            .unwrap();
        for index in 2..=41 {
            let link_name = alloc::format!("/little_file_system_chain/s{index:02}");
            let previous_name = alloc::format!("s{:02}", index - 1);
            virtual_file_system
                .create_symlink(
                    task,
                    Path::from_str(&previous_name),
                    Path::from_str(&link_name),
                )
                .await
                .unwrap();
        }
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("self_loop"),
                Path::from_str("/little_file_system_chain/self_loop"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("loop_b"),
                Path::from_str("/little_file_system_chain/loop_a"),
            )
            .await
            .unwrap();
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("loop_a"),
                Path::from_str("/little_file_system_chain/loop_b"),
            )
            .await
            .unwrap();

        let little_file_system_root = virtual_file_system
            .resolve_directory_root(task, &"/little_file_system_chain")
            .await
            .unwrap();
        let mut at_40 = virtual_file_system
            .open_beneath(
                &"s40",
                Flags::from(AccessFlags::Read),
                task,
                &little_file_system_root,
            )
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut at_40).await, b"at-bound");

        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"s41",
                    Flags::from(AccessFlags::Read),
                    task,
                    &little_file_system_root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );
        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"self_loop",
                    Flags::from(AccessFlags::Read),
                    task,
                    &little_file_system_root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );
        assert_eq!(
            virtual_file_system
                .open_beneath(
                    &"loop_a",
                    Flags::from(AccessFlags::Read),
                    task,
                    &little_file_system_root,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );

        // Verify the iterative resolver executes zero allocations proportional
        // to the number of followed symlinks (2 links vs 40 equal-length links).
        let simulated = SimulatedDirectoryEntryFileSystem::new();
        let sandbox_inode = simulated.populate_directory(1, "sandbox").unwrap();
        simulated
            .populate_file(sandbox_inode, "end", b"ok")
            .unwrap();
        simulated
            .populate_symlink(sandbox_inode, "l01", "end")
            .unwrap();
        for index in 2..=41 {
            let name = alloc::format!("l{index:02}");
            let target = alloc::format!("l{:02}", index - 1);
            simulated
                .populate_symlink(sandbox_inode, &name, &target)
                .unwrap();
        }

        virtual_file_system
            .create_directory(task, &"/simulated")
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(simulated, &"/simulated", task)
            .await
            .unwrap();
        let simulated_root = virtual_file_system
            .resolve_directory_root(task, &"/simulated/sandbox")
            .await
            .unwrap();

        let two_link_allocations = allocation::measure(|| {
            let file = poll_uncontended(virtual_file_system.open_beneath_with_lookup(
                &"l02",
                Flags::from(AccessFlags::Read),
                task,
                &simulated_root,
                true,
            ))
            .unwrap();
            drop(file);
        });
        let forty_link_allocations = allocation::measure(|| {
            let file = poll_uncontended(virtual_file_system.open_beneath_with_lookup(
                &"l40",
                Flags::from(AccessFlags::Read),
                task,
                &simulated_root,
                true,
            ))
            .unwrap();
            drop(file);
        });
        assert_eq!(forty_link_allocations, two_link_allocations);

        assert_eq!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"l41",
                    Flags::from(AccessFlags::Read),
                    task,
                    &simulated_root,
                    true,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );
    }

    #[cfg(target_os = "linux")]
    #[task::test]
    async fn directory_entry_backend_confines_across_renames_and_rejects_path_fallbacks() {
        let (task, virtual_file_system) = initialize().await;

        let simulated = SimulatedDirectoryEntryFileSystem::new();
        simulated
            .populate_file(1, "outside_secret.txt", b"escaped")
            .unwrap();
        let sandbox_inode = simulated.populate_directory(1, "sandbox").unwrap();
        let active_inode = simulated
            .populate_directory(sandbox_inode, "active")
            .unwrap();
        simulated
            .populate_file(active_inode, "secret.txt", b"confined-secret")
            .unwrap();
        simulated
            .populate_file(sandbox_inode, "victim.txt", b"original")
            .unwrap();
        simulated
            .populate_directory(sandbox_inode, "victim_directory")
            .unwrap();

        virtual_file_system
            .create_directory(task, &"/host")
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(simulated, &"/host", task)
            .await
            .unwrap();

        let root = virtual_file_system
            .resolve_directory_root(task, &"/host/sandbox")
            .await
            .unwrap();

        // Exercise every namespace and directory operation through pinned entry
        // handles, including opening `.` and `subdirectory/..` beneath root.
        assert!(
            virtual_file_system
                .open_directory_beneath(task, &".", &root)
                .await
                .is_ok()
        );
        virtual_file_system
            .create_directory_beneath(task, &"scratch", &root)
            .await
            .unwrap();
        assert!(
            virtual_file_system
                .open_directory_beneath(task, &"scratch/..", &root)
                .await
                .is_ok()
        );
        virtual_file_system
            .create_symlink_beneath(
                task,
                Path::from_str("secret.txt"),
                Path::from_str("active/link_to_secret"),
                &root,
            )
            .await
            .unwrap();
        let mut target_buffer = [0u8; 32];
        let target_length = virtual_file_system
            .read_symlink_beneath(
                task,
                Path::from_str("active/link_to_secret"),
                &mut target_buffer,
                &root,
            )
            .await
            .unwrap();
        assert_eq!(&target_buffer[..target_length], b"secret.txt");

        let statistics = virtual_file_system
            .get_statistics_beneath(task, &"active/link_to_secret", false, &root)
            .await
            .unwrap();
        assert_eq!(statistics.kind, Kind::SymbolicLink);

        virtual_file_system
            .rename_beneath(
                task,
                &"active/link_to_secret",
                &"scratch/moved_link",
                &root,
                &root,
            )
            .await
            .unwrap();
        virtual_file_system
            .remove_beneath(task, &"scratch/moved_link", &root)
            .await
            .unwrap();
        virtual_file_system
            .remove_beneath(task, &"scratch", &root)
            .await
            .unwrap();

        // Case 1: rename `active` and replace its slot with an escaping symlink
        // between attribute check and lookup_file_entry. Pinned parent Context
        // must open the original moved directory's file, not `/outside_secret.txt`.
        simulated.schedule_hook_on_entry_check(
            "secret.txt",
            EntrySwapAction::RenameDirectoryAndReplaceWithSymlink {
                parent_inode: sandbox_inode,
                old_name: String::from("active"),
                new_name: String::from("active_moved"),
                symlink_target: String::from(".."),
            },
            true,
        );
        let mut confined_file = virtual_file_system
            .open_beneath_with_lookup(
                &"active/secret.txt",
                Flags::from(AccessFlags::Read),
                task,
                &root,
                false,
            )
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut confined_file).await, b"confined-secret");
        drop(confined_file);

        // Case 2: rename `active_moved` and replace its slot with an escaping
        // symlink right before creating `created.txt`; the pinned parent Context
        // confines file creation inside `active_second`, not outside `/host/sandbox`.
        simulated.schedule_hook_on_entry_check(
            "created.txt",
            EntrySwapAction::RenameDirectoryAndReplaceWithSymlink {
                parent_inode: sandbox_inode,
                old_name: String::from("active_moved"),
                new_name: String::from("active_second"),
                symlink_target: String::from(".."),
            },
            false,
        );
        let mut created_file = virtual_file_system
            .open_beneath_with_lookup(
                &"active_moved/created.txt",
                Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
                task,
                &root,
                false,
            )
            .await
            .unwrap();
        created_file
            .write(b"created-in-pinned-parent")
            .await
            .unwrap();
        drop(created_file);
        assert_eq!(
            virtual_file_system
                .open(&"/host/created.txt", Flags::from(AccessFlags::Read), task)
                .await
                .unwrap_err(),
            IoErrorKind::NotFound
        );
        let mut verified_created = virtual_file_system
            .open(
                &"/host/sandbox/active_second/created.txt",
                Flags::from(AccessFlags::Read),
                task,
            )
            .await
            .unwrap();
        assert_eq!(
            read_to_vec(&mut verified_created).await,
            b"created-in-pinned-parent"
        );

        // Case 3: swap the final file entry to a symlink between VFS attribute
        // check and backend lookup_file_entry; no-follow lookup_file_entry rejects it.
        simulated.schedule_hook_on_entry_check(
            "victim.txt",
            EntrySwapAction::ReplaceEntryWithSymlink {
                parent_inode: sandbox_inode,
                name: String::from("victim.txt"),
                symlink_target: String::from("../outside_secret.txt"),
            },
            true,
        );
        assert_eq!(
            virtual_file_system
                .open_beneath_with_lookup(
                    &"victim.txt",
                    Flags::from(AccessFlags::Read),
                    task,
                    &root,
                    false,
                )
                .await
                .unwrap_err(),
            IoErrorKind::SymlinkLoop
        );

        // Case 4: swap the final directory entry to a symlink between VFS
        // attribute check and backend lookup_directory_entry.
        simulated.schedule_hook_on_entry_check(
            "victim_directory",
            EntrySwapAction::ReplaceEntryWithSymlink {
                parent_inode: sandbox_inode,
                name: String::from("victim_directory"),
                symlink_target: String::from(".."),
            },
            true,
        );
        assert_eq!(
            virtual_file_system
                .open_directory_beneath_with_lookup(task, &"victim_directory", &root, false)
                .await
                .err()
                .unwrap(),
            IoErrorKind::SymlinkLoop
        );

        // Cleanly close opened directories when traversing cross-mount into this backend.
        virtual_file_system
            .create_directory(task, &"/cross_sandbox")
            .await
            .unwrap();
        write_new_file(
            virtual_file_system,
            task,
            "/cross_sandbox/little_file_system_inside.txt",
            b"from-little-file-system",
        )
        .await;
        virtual_file_system
            .create_symlink(
                task,
                Path::from_str("host_subdirectory/active_moved/secret.txt"),
                Path::from_str("/cross_sandbox/into_host"),
            )
            .await
            .unwrap();

        let cross_simulated = SimulatedDirectoryEntryFileSystem::new();
        let nested_sandbox = cross_simulated
            .populate_directory(1, "active_moved")
            .unwrap();
        cross_simulated
            .populate_file(nested_sandbox, "secret.txt", b"across-mounts")
            .unwrap();
        cross_simulated
            .populate_symlink(
                1,
                "back_to_little_file_system",
                "../little_file_system_inside.txt",
            )
            .unwrap();
        virtual_file_system
            .create_directory(task, &"/cross_sandbox/host_subdirectory")
            .await
            .unwrap();
        virtual_file_system
            .mount_file_system(cross_simulated, &"/cross_sandbox/host_subdirectory", task)
            .await
            .unwrap();

        let cross_root = virtual_file_system
            .resolve_directory_root(task, &"/cross_sandbox")
            .await
            .unwrap();
        let mut cross_file = virtual_file_system
            .open_beneath_with_lookup(
                &"into_host",
                Flags::from(AccessFlags::Read),
                task,
                &cross_root,
                true,
            )
            .await
            .unwrap();
        assert_eq!(read_to_vec(&mut cross_file).await, b"across-mounts");

        let mut back_file = virtual_file_system
            .open_beneath_with_lookup(
                &"host_subdirectory/back_to_little_file_system",
                Flags::from(AccessFlags::Read),
                task,
                &cross_root,
                true,
            )
            .await
            .unwrap();
        assert_eq!(
            read_to_vec(&mut back_file).await,
            b"from-little-file-system"
        );
    }
}
