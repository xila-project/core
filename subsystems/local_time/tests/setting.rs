//! The time zone setting against a real virtual file system on LittleFS: the
//! start-up with no file, a valid IANA name, a custom TZ string, a corrupt file
//! and a file not written by root, then changes through `set`.
#![cfg(target_os = "linux")]

testing_support::instantiate_global_allocator!();

extern crate alloc;

use alloc::string::String;
use file_system::{AccessFlags, CreateFlags, Flags, MemoryDevice, Permissions};
use local_time::{CONFIGURATION_PATH, ENVIRONMENT_VARIABLE, Setting};
use task::{TaskIdentifier, test};
use users::{ADMINISTRATOR_GROUP_NAME, GroupIdentifier, UserIdentifier};
use virtual_file_system::{File, VirtualFileSystem};

const PARIS: &str = "CET-1CEST,M3.5.0,M10.5.0/3";

async fn boot() -> (TaskIdentifier, &'static VirtualFileSystem) {
    if !log::is_initialized() {
        let _ = log::initialize(&testing_support::Logger);
    }
    let task_manager = task::test_manager();
    let task = task_manager.get_current_task_identifier().await;
    let users = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let disk = MemoryDevice::<512>::new_static(256 * 512);
    let file_system =
        little_fs::FileSystem::get_or_format(disk, little_fs::Options::new(256)).unwrap();
    let vfs: &'static VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(task_manager, users, time, file_system).unwrap(),
    ));
    virtual_file_system::create_default_hierarchy(vfs, task)
        .await
        .unwrap();
    (task, vfs)
}

async fn write_configuration(vfs: &VirtualFileSystem, task: TaskIdentifier, contents: &[u8]) {
    File::write_to_path(vfs, task, CONFIGURATION_PATH, contents)
        .await
        .unwrap();
}

async fn variable(task_manager: &task::Manager, task: TaskIdentifier) -> Option<String> {
    task_manager
        .get_environment_variable(task, ENVIRONMENT_VARIABLE)
        .await
        .ok()
        .map(|variable| String::from(variable.get_value()))
}

/// Run `body` in a child task of `parent` that runs as `user`.
async fn as_user<F>(parent: TaskIdentifier, user: UserIdentifier, body: F)
where
    F: AsyncFnOnce(TaskIdentifier) + 'static,
{
    let manager = task::test_manager();
    let spawner = manager.get_spawner(parent).await.unwrap();
    manager
        .spawn(parent, "User", Some(spawner), async move |child| {
            task::test_manager().set_user(child, user).await.unwrap();
            body(child).await;
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test]
async fn start_up_without_a_file_uses_utc() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    assert_eq!(
        local_time::read(vfs, task).await,
        Err(file_system::IoErrorKind::NotFound)
    );
    assert_eq!(
        local_time::initialize(manager, vfs, task).await,
        Setting::utc()
    );
    assert_eq!(variable(manager, task).await.as_deref(), Some("UTC0"));
    let zone = local_time::current(manager, task).await;
    assert_eq!(zone.standard_abbreviation(), "UTC");
    assert_eq!(zone.offset_at(1_721_044_800), 0);
}

#[test]
async fn start_up_with_an_iana_name_publishes_its_string() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    write_configuration(
        vfs,
        task,
        br#"{"name":"Europe/Paris","time_zone":"CET-1CEST,M3.5.0,M10.5.0/3"}"#,
    )
    .await;
    let setting = local_time::initialize(manager, vfs, task).await;
    assert_eq!(setting.name(), Some("Europe/Paris"));
    assert_eq!(variable(manager, task).await.as_deref(), Some(PARIS));

    // A task started afterwards inherits the variable, as a native executable
    // or a WebAssembly guest does.
    let spawner = manager.get_spawner(task).await.unwrap();
    manager
        .spawn(task, "Child", Some(spawner), async move |child| {
            let manager = task::test_manager();
            assert_eq!(variable(manager, child).await.as_deref(), Some(PARIS));
            let zone = local_time::current(manager, child).await;
            let local = zone.from_utc(1_721_044_800);
            assert_eq!(local.abbreviation, "CEST");
            assert_eq!(local.offset_seconds, 7200);
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
}

#[test]
async fn start_up_with_a_custom_string() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    write_configuration(vfs, task, br#"{"time_zone":"<+0545>-5:45"}"#).await;
    let setting = local_time::initialize(manager, vfs, task).await;
    assert_eq!(setting.name(), None);
    assert_eq!(
        variable(manager, task).await.as_deref(),
        Some("<+0545>-5:45")
    );
    assert_eq!(
        local_time::current(manager, task).await.offset_at(0),
        5 * 3600 + 45 * 60
    );
}

#[test]
async fn start_up_with_a_corrupt_file_uses_utc() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    for contents in [
        &b"Europe/Paris"[..],
        br#"{"time_zone":"not a TZ string"}"#,
        br#"{"name":"Europe/Paris"}"#,
        b"\xff\xfe",
        &[b' '; local_time::MAXIMUM_CONFIGURATION_SIZE + 1],
    ] {
        write_configuration(vfs, task, contents).await;
        assert!(local_time::read(vfs, task).await.is_err());
        assert_eq!(
            local_time::initialize(manager, vfs, task).await,
            Setting::utc()
        );
        assert_eq!(variable(manager, task).await.as_deref(), Some("UTC0"));
    }
}

#[test]
async fn start_up_ignores_a_file_that_others_can_write() {
    let (task, vfs) = boot().await;
    write_configuration(vfs, task, br#"{"time_zone":"EST5EDT,M3.2.0,M11.1.0"}"#).await;
    let mut file = File::open(vfs, task, CONFIGURATION_PATH, AccessFlags::Write.into())
        .await
        .unwrap();
    file.set_permissions(Permissions::ALL_READ_WRITE)
        .await
        .unwrap();
    file.close(vfs).await.unwrap();
    assert_eq!(
        local_time::read(vfs, task).await,
        Err(file_system::IoErrorKind::PermissionDenied)
    );
    assert_eq!(local_time::load(vfs, task).await, Setting::utc());
}

#[test]
async fn set_writes_the_file_and_updates_the_tasks_that_follow_it() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    let users = vfs.users_manager();
    local_time::initialize(manager, vfs, task).await;

    let paris = Setting::new(Some("Europe/Paris"), PARIS).unwrap();
    local_time::set(manager, users, vfs, task, &paris)
        .await
        .unwrap();
    assert_eq!(local_time::read(vfs, task).await.unwrap(), paris);
    assert_eq!(variable(manager, task).await.as_deref(), Some(PARIS));
    // The file belongs to root and everyone can read it.
    let statistics = vfs
        .get_statistics(&file_system::Path::from_str(CONFIGURATION_PATH))
        .await
        .unwrap();
    assert_eq!(statistics.user, UserIdentifier::ROOT);
    assert_eq!(
        statistics.permissions,
        Permissions::from_octal(0o644).unwrap()
    );
    // No temporary file is left.
    assert!(
        File::open(
            vfs,
            task,
            "/configuration/system/.time_zone.json-new",
            AccessFlags::Read.into()
        )
        .await
        .is_err()
    );

    // A running task that follows the system value sees the change; a task
    // with its own TZ keeps it.
    let spawner = manager.get_spawner(task).await.unwrap();
    manager
        .spawn(task, "Follower", Some(spawner), async move |follower| {
            let manager = task::test_manager();
            let users = vfs.users_manager();
            let spawner = manager.get_spawner(follower).await.unwrap();
            manager
                .spawn(follower, "Own", Some(spawner), async move |own| {
                    let manager = task::test_manager();
                    manager
                        .set_environment_variable(own, ENVIRONMENT_VARIABLE, "JST-9")
                        .await
                        .unwrap();
                    let custom = Setting::new(None, "<+0545>-5:45").unwrap();
                    local_time::set(manager, users, vfs, own, &custom)
                        .await
                        .unwrap();
                    assert_eq!(variable(manager, own).await.as_deref(), Some("JST-9"));
                })
                .await
                .unwrap()
                .0
                .join()
                .await;
            assert_eq!(
                variable(manager, follower).await.as_deref(),
                Some("<+0545>-5:45")
            );
        })
        .await
        .unwrap()
        .0
        .join()
        .await;
    let stored = local_time::read(vfs, task).await.unwrap();
    assert_eq!(stored.name(), None);
    assert_eq!(stored.time_zone_string(), "<+0545>-5:45");
}

#[test]
async fn set_needs_root_or_an_administrator() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    let users = vfs.users_manager();
    local_time::initialize(manager, vfs, task).await;

    let group = GroupIdentifier::new(1001);
    users.add_group(group, "users", &[]).await.unwrap();
    let user = UserIdentifier::new(1001);
    users
        .add_user(user, "user", group, "/users/user")
        .await
        .unwrap();
    let administrators = GroupIdentifier::new(1002);
    users
        .add_group(administrators, ADMINISTRATOR_GROUP_NAME, &[])
        .await
        .unwrap();
    let administrator = UserIdentifier::new(1002);
    users
        .add_user(
            administrator,
            "administrator",
            group,
            "/users/administrator",
        )
        .await
        .unwrap();
    users
        .add_to_group(administrator, administrators)
        .await
        .unwrap();

    as_user(task, user, async move |child| {
        let setting = Setting::new(None, "JST-9").unwrap();
        assert_eq!(
            local_time::set(
                vfs.task_manager(),
                vfs.users_manager(),
                vfs,
                child,
                &setting
            )
            .await,
            Err(file_system::IoErrorKind::PermissionDenied)
        );
        // Everyone can read the setting.
        assert_eq!(local_time::load(vfs, child).await, Setting::utc());
    })
    .await;
    assert_eq!(variable(manager, task).await.as_deref(), Some("UTC0"));

    as_user(task, administrator, async move |child| {
        let setting = Setting::new(Some("Asia/Tokyo"), "JST-9").unwrap();
        local_time::set(
            vfs.task_manager(),
            vfs.users_manager(),
            vfs,
            child,
            &setting,
        )
        .await
        .unwrap();
    })
    .await;
    assert_eq!(variable(manager, task).await.as_deref(), Some("JST-9"));
    let statistics = vfs
        .get_statistics(&file_system::Path::from_str(CONFIGURATION_PATH))
        .await
        .unwrap();
    assert_eq!(statistics.user, UserIdentifier::ROOT);
}

#[test]
async fn a_failed_write_keeps_the_old_setting() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    let users = vfs.users_manager();
    let paris = Setting::new(Some("Europe/Paris"), PARIS).unwrap();
    local_time::set(manager, users, vfs, task, &paris)
        .await
        .unwrap();
    // A directory in place of the temporary file makes the write fail.
    vfs.create_directory(task, &"/configuration/system/.time_zone.json-new")
        .await
        .unwrap();
    let placeholder = File::open(
        vfs,
        task,
        "/configuration/system/.time_zone.json-new/keep",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    placeholder.close(vfs).await.ok();
    let tokyo = Setting::new(Some("Asia/Tokyo"), "JST-9").unwrap();
    assert!(
        local_time::set(manager, users, vfs, task, &tokyo)
            .await
            .is_err()
    );
    assert_eq!(local_time::read(vfs, task).await.unwrap(), paris);
    assert_eq!(variable(manager, task).await.as_deref(), Some(PARIS));
}

#[test]
async fn concurrent_sets_leave_the_file_and_tz_in_agreement() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    let users = vfs.users_manager();
    local_time::initialize(manager, vfs, task).await;
    let paris = Setting::new(Some("Europe/Paris"), PARIS).unwrap();
    let tokyo = Setting::new(Some("Asia/Tokyo"), "JST-9").unwrap();
    for _ in 0..8 {
        let (first, second) = embassy_futures::join::join(
            local_time::set(manager, users, vfs, task, &paris),
            local_time::set(manager, users, vfs, task, &tokyo),
        )
        .await;
        first.unwrap();
        second.unwrap();
        // The later call wins in both places: the file and `TZ` agree.
        let stored = local_time::read(vfs, task).await.unwrap();
        assert_eq!(
            variable(manager, task).await.as_deref(),
            Some(stored.time_zone_string())
        );
    }
}

#[test]
async fn the_setting_in_use_is_kept_in_ram() {
    let (task, vfs) = boot().await;
    let manager = vfs.task_manager();
    let users = vfs.users_manager();
    write_configuration(vfs, task, br#"{"name":"Asia/Tokyo","time_zone":"JST-9"}"#).await;
    local_time::initialize(manager, vfs, task).await;
    assert_eq!(local_time::stored().name(), Some("Asia/Tokyo"));
    let paris = Setting::new(Some("Europe/Paris"), PARIS).unwrap();
    local_time::set(manager, users, vfs, task, &paris)
        .await
        .unwrap();
    assert_eq!(local_time::stored(), paris);
}
