#![cfg(target_os = "linux")]
extern crate abi_definitions;
extern crate alloc;
drivers_std::memory::instantiate_global_allocator!();
use graphics::{FilePickerMode, FilePickerStatus};
#[path = "../src/file_picker/model.rs"]
mod model;

#[task::test]
async fn privileged_worker_cannot_browse_or_select_for_unprivileged_caller() {
    use file_system::{AccessFlags, CreateFlags, Flags, MemoryDevice, Permissions};
    use model::{Browser, Model};
    use virtual_file_system::{Error, File, VirtualFileSystem};
    let manager = task::test_manager();
    let caller = manager.get_current_task_identifier().await;
    let users = Box::leak(Box::new(users::Manager::new()));
    let time = Box::leak(Box::new(
        time::Manager::new(&drivers_std::devices::TimeDevice).unwrap(),
    ));
    let device = MemoryDevice::<512>::new_static(1024 * 512);
    little_fs::FileSystem::format(device, 256).unwrap();
    let fs = little_fs::FileSystem::new(device, 256).unwrap();
    let vfs: &'static VirtualFileSystem = Box::leak(Box::new(
        virtual_file_system::initialize(manager, users, time, fs).unwrap(),
    ));
    vfs.set_permissions(caller, "/", Permissions::from_octal(0o755).unwrap())
        .await
        .unwrap();
    vfs.create_directory(caller, &"/private").await.unwrap();
    File::open(
        vfs,
        caller,
        "/private/file",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap()
    .close(vfs)
    .await
    .unwrap();
    vfs.set_permissions(caller, "/private", Permissions::from_octal(0o700).unwrap())
        .await
        .unwrap();
    let mut retained = File::open(
        vfs,
        caller,
        "/retained",
        Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap();
    retained.write(b"preserve me").await.unwrap();
    retained.close(vfs).await.unwrap();
    vfs.set_permissions(caller, "/retained", Permissions::from_octal(0o666).unwrap())
        .await
        .unwrap();
    File::open(
        vfs,
        caller,
        "/restricted",
        Flags::new(AccessFlags::Write, Some(CreateFlags::Create), None),
    )
    .await
    .unwrap()
    .close(vfs)
    .await
    .unwrap();
    vfs.set_permissions(
        caller,
        "/restricted",
        Permissions::from_octal(0o000).unwrap(),
    )
    .await
    .unwrap();
    let group = users::GroupIdentifier::new(1000);
    let user = users::UserIdentifier::new(1000);
    users.add_group(group, "picker-user", &[]).await.unwrap();
    users.add_user(user, "picker-user", group).await.unwrap();
    let spawner = manager.get_spawner(caller).await.unwrap();
    manager.set_user(caller, user).await.unwrap();
    manager
        .spawn(
            caller,
            "privileged graphics worker",
            Some(spawner),
            async move |worker| {
                manager
                    .set_user(worker, users::UserIdentifier::ROOT)
                    .await
                    .unwrap();
                assert_ne!(worker, caller);
                assert!(
                    vfs.page(worker, "/private", 0).is_ok(),
                    "worker itself has permission"
                );
                let mut model = Model::new(caller, FilePickerMode::OpenFile);
                model.navigate(vfs, "/", 0).unwrap();
                assert_eq!(
                    model.navigate(vfs, "/private", 0),
                    Err(Error::PermissionDenied)
                );
                assert_eq!(model.directory, "/");
                assert_eq!(
                    model.confirm(vfs, "/private/file"),
                    Err(Error::PermissionDenied)
                );
                assert_eq!(model.status, FilePickerStatus::Pending);
                assert!(model.selected.is_none());
                assert_eq!(
                    vfs.kind(caller, "/restricted").unwrap(),
                    file_system::Kind::File,
                    "metadata alone does not prove file access"
                );
                assert_eq!(
                    model.confirm(vfs, "/restricted"),
                    Err(Error::PermissionDenied)
                );
                let mut directory = Model::new(caller, FilePickerMode::Directory);
                assert_eq!(
                    directory.confirm(vfs, "/private"),
                    Err(Error::PermissionDenied)
                );
                let mut save = Model::new(caller, FilePickerMode::SaveFile);
                assert_eq!(
                    save.confirm(vfs, "/restricted"),
                    Err(Error::PermissionDenied)
                );
                assert_eq!(
                    save.confirm(vfs, "/private/new"),
                    Err(Error::PermissionDenied)
                );
                assert!(matches!(
                    vfs.kind(worker, "/private/new"),
                    Err(Error::FileSystem(file_system::Error::NotFound))
                ));
                let mut save = Model::new(caller, FilePickerMode::SaveFile);
                save.confirm(vfs, "/retained").unwrap();
                assert_eq!(save.selected.as_deref(), Some("/retained"));
                let mut retained = File::open(vfs, worker, "/retained", AccessFlags::Read.into())
                    .await
                    .unwrap();
                let mut bytes = [0; 11];
                assert_eq!(retained.read(&mut bytes).await.unwrap(), 11);
                assert_eq!(&bytes, b"preserve me");
                retained.close(vfs).await.unwrap();
            },
        )
        .await
        .unwrap()
        .0
        .join()
        .await;
    manager
        .set_user(caller, users::UserIdentifier::ROOT)
        .await
        .unwrap();
}
