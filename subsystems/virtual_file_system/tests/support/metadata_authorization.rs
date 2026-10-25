use super::*;
use file_system::Permissions;
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{Error, SynchronousFile};

async fn fixture() -> (TaskIdentifier, &'static VirtualFileSystem, UserIdentifier) {
    let (task, vfs) = initialize().await;
    vfs.set_permissions(task, "/", Permissions::DIRECTORY_DEFAULT)
        .await
        .unwrap();
    let user = UserIdentifier::new(1001);
    vfs.users_manager()
        .add_user(user, "owner", GroupIdentifier::ROOT)
        .await
        .unwrap();
    File::write_to_path(vfs, task, "/file", b"data")
        .await
        .unwrap();
    (task, vfs, user)
}

#[task::test]
async fn readable_foreign_handle_and_duplicate_cannot_mutate_metadata() {
    let (task, vfs, user) = fixture().await;
    let mut file = File::open(vfs, task, "/file", AccessFlags::Read.into())
        .await
        .unwrap();
    let mut duplicate = file.duplicate().await.unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert_eq!(
        file.set_permissions(Permissions::ALL_FULL).await,
        Err(Error::PermissionDenied)
    );
    assert_eq!(
        duplicate.set_owner(Some(user), None).await,
        Err(Error::PermissionDenied)
    );
    assert_eq!(
        duplicate
            .as_synchronous_file_mut()
            .set_permissions(Permissions::ALL_FULL),
        Err(Error::PermissionDenied)
    );
}

#[task::test]
async fn retained_authority_tracks_demotion_and_current_owner() {
    let (task, vfs, user) = fixture().await;
    vfs.set_ownership(task, "/file", Some(user), None)
        .await
        .unwrap();
    let mut file = File::open(vfs, task, "/file", AccessFlags::READ_WRITE.into())
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    file.set_permissions(Permissions::USER_READ_WRITE)
        .await
        .unwrap();
    vfs.task_manager()
        .set_user(task, UserIdentifier::ROOT)
        .await
        .unwrap();
    vfs.set_ownership(task, "/file", Some(UserIdentifier::ROOT), None)
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert_eq!(
        file.set_permissions(Permissions::ALL_FULL).await,
        Err(Error::PermissionDenied)
    );
    vfs.task_manager()
        .set_user(task, UserIdentifier::ROOT)
        .await
        .unwrap();
    file.close(vfs).await.unwrap();
    assert_eq!(
        vfs.get_statistics(&"/file").await.unwrap().user,
        UserIdentifier::ROOT
    );
}

#[task::test]
async fn root_fd_ownership_is_explicitly_unsupported_and_path_chown_is_root_only() {
    let (task, vfs, user) = fixture().await;
    let mut file = File::open(vfs, task, "/file", AccessFlags::READ_WRITE.into())
        .await
        .unwrap();
    assert_eq!(
        file.set_owner(Some(user), None).await,
        Err(Error::UnsupportedOperation)
    );
    assert_eq!(
        file.set_owner(None, Some(GroupIdentifier::ROOT)).await,
        Err(Error::UnsupportedOperation)
    );
    file.set_owner(None, None).await.unwrap();
    vfs.set_ownership(task, "/file", Some(user), None)
        .await
        .unwrap();
    vfs.task_manager().set_user(task, user).await.unwrap();
    assert_eq!(
        vfs.set_ownership(task, "/file", Some(UserIdentifier::ROOT), None)
            .await,
        Err(Error::PermissionDenied)
    );
    assert_eq!(
        file.set_owner(Some(UserIdentifier::ROOT), None).await,
        Err(Error::PermissionDenied)
    );
}

#[task::test]
async fn root_readonly_littlefs_chmod_is_explicitly_unsupported() {
    let (task, vfs, _) = fixture().await;
    let mut file = File::open(vfs, task, "/file", AccessFlags::Read.into())
        .await
        .unwrap();
    let before = vfs.get_statistics(&"/file").await.unwrap().permissions;
    assert_eq!(
        file.set_permissions(Permissions::ALL_FULL).await,
        Err(Error::FileSystem(file_system::Error::UnsupportedOperation))
    );
    assert_eq!(
        vfs.get_statistics(&"/file").await.unwrap().permissions,
        before
    );
}

#[task::test]
async fn metadata_allocation_measurement() {
    let (task, vfs, _) = fixture().await;
    let mut file = None;
    let opens = allocation::measure(|| {
        file = Some(
            task::block_on(File::open(
                vfs,
                task,
                "/file",
                AccessFlags::READ_WRITE.into(),
            ))
            .unwrap(),
        )
    });
    let mut file = file.unwrap();
    let chmod = allocation::measure(|| {
        task::block_on(file.set_permissions(Permissions::USER_READ_WRITE)).unwrap()
    });
    let chown = allocation::measure(|| {
        task::block_on(vfs.set_ownership(task, "/file", Some(UserIdentifier::ROOT), None)).unwrap()
    });
    let mut byte = [0];
    let read = allocation::measure(|| {
        task::block_on(file.read(&mut byte)).unwrap();
    });
    let write = allocation::measure(|| {
        task::block_on(file.write(b"x")).unwrap();
    });
    std::eprintln!(
        "metadata measurement: sync_file={} open_allocs={opens} chmod_allocs={chmod} chown_allocs={chown} read_allocs={read} write_allocs={write}",
        core::mem::size_of::<SynchronousFile<'_>>()
    );
    assert_eq!(read, 0);
    assert_eq!(write, 0);
}
