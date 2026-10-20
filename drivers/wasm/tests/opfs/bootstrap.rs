use super::*;
use file_system::{
    XILA_DISK_SIGNATURE,
    mbr::{Mbr, PartitionKind},
};

#[wasm_bindgen_test]
async fn existing_invalid_mbr_is_not_reformatted() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
        .await
        .unwrap();
    drive.write_sync(&[0x42; 512], 0).unwrap();
    drive.sync().await.unwrap();
    drop(drive);
    let drive = Box::leak(Box::new(
        DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
            .await
            .unwrap(),
    ));
    assert!(
        storage::mount(drive).is_err(),
        "Existing invalid MBR must not trigger formatting"
    );
    drive.sync().await.unwrap();
    let mut bytes = [0; 512];
    drive.read_sync(&mut bytes, 0).unwrap();
    assert_eq!(bytes, [0x42; 512]);
    drive.unmount().unwrap();
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn existing_unmountable_filesystem_is_not_reformatted() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
        .await
        .unwrap();
    let partition = Mbr::format_disk_with_signature_and_partition(
        &drive,
        XILA_DISK_SIGNATURE,
        PartitionKind::Xila,
    )
    .unwrap();
    partition.write_sync(&[0x42; 1024], 0).unwrap();
    drive.sync().await.unwrap();
    drop(drive);
    let drive = Box::leak(Box::new(
        DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
            .await
            .unwrap(),
    ));
    assert!(
        storage::mount(drive).is_err(),
        "Existing invalid filesystem must not trigger formatting"
    );
    drive.sync().await.unwrap();
    let mut bytes = [0; 1024];
    drive.read_sync(&mut bytes, 2048 * 512).unwrap();
    assert_eq!(bytes, [0x42; 1024]);
    drive.unmount().unwrap();
    remove(&name).await;
}

type Completion = synchronization::signal::Signal<
    synchronization::blocking_mutex::raw::CriticalSectionRawMutex,
    Result<(), String>,
>;

#[embassy_executor::task]
async fn check_saved_accounts(completion: &'static Completion) {
    completion.signal(saved_accounts_fixture().await);
}

async fn saved_accounts_fixture() -> Result<(), String> {
    use xila::{authentication, task, time, users, virtual_file_system};
    let name = name();
    let drive = Box::leak(Box::new(
        DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
            .await
            .map_err(|e| e.to_string())?,
    ));
    let partition = Mbr::format_disk_with_signature_and_partition(
        drive,
        XILA_DISK_SIGNATURE,
        PartitionKind::Xila,
    )
    .map_err(|e| e.to_string())?;
    drop(
        little_fs::FileSystem::new_format(Box::leak(Box::new(partition)), 256)
            .map_err(|e| e.to_string())?,
    );
    let task_manager = Box::leak(Box::new(task::Manager::new()));
    let task = task_manager.get_current_task_identifier().await;
    let users_manager = Box::leak(Box::new(users::Manager::new()));
    let time_manager = Box::leak(Box::new(
        time::Manager::new(&drivers_wasm::devices::TimeDevice).map_err(|e| e.to_string())?,
    ));
    let vfs = virtual_file_system::initialize(
        task_manager,
        users_manager,
        time_manager,
        storage::mount(drive).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    virtual_file_system::create_default_hierarchy(&vfs, task)
        .await
        .map_err(|e| e.to_string())?;
    for directory in ["/system/accounts/groups", "/system/accounts/users"] {
        vfs.create_directory(task, &directory)
            .await
            .map_err(|e| e.to_string())?;
    }
    let group = br#"{"identifier":1000,"name":"administrator","users":[42]}"#;
    let user = br#"{"identifier":42,"name":"bob","primary_group":1000,"hash":"saved-hash","salt":"saved-salt"}"#;
    for (path, bytes) in [
        ("/system/accounts/groups/administrator", group.as_slice()),
        ("/system/accounts/users/bob", user.as_slice()),
    ] {
        virtual_file_system::File::write_to_path(&vfs, task, &path, bytes)
            .await
            .map_err(|e| e.to_string())?;
    }
    let context = authentication::Context {
        virtual_file_system: &vfs,
        task_manager,
        users_manager,
        task,
    };
    storage::initialize_users(&context, false)
        .await
        .map_err(|e| e.to_string())?;
    let loaded = users_manager
        .get_user_name(users::UserIdentifier::new(42))
        .await
        .map_err(|e| e.to_string())?;
    if loaded != "bob" {
        return Err("Saved non-administrator user was not loaded".into());
    }
    let mut bytes = Vec::new();
    virtual_file_system::File::read_from_path(
        &vfs,
        task,
        &"/system/accounts/groups/administrator",
        &mut bytes,
    )
    .await
    .map_err(|e| e.to_string())?;
    if bytes != group {
        return Err("Saved administrator group was overwritten".into());
    }
    vfs.uninitialize().await;
    drop(vfs);
    drive.sync().await.map_err(|e| e.to_string())?;
    drive.unmount().map_err(|e| e.to_string())?;
    remove(&name).await;
    Ok(())
}

#[wasm_bindgen_test]
async fn saved_accounts_are_loaded_even_without_an_administrator_file() {
    let completion = Box::leak(Box::new(Completion::new()));
    let executor = Box::leak(Box::new(drivers_wasm::executor::Executor::new()));
    executor.start(|spawner| spawner.spawn(check_saved_accounts(completion).unwrap()));
    assert_eq!(completion.wait().await, Ok(()));
}

#[wasm_bindgen_test]
async fn new_placeholder_imports_a_seed_and_reopens_without_a_download() {
    use file_system::{AccessFlags, BaseOperations, Context, FileSystemOperations};
    let source_name = name();
    let source = Box::leak(Box::new(
        DriveDevice::new(Path::from_str(&source_name), 4 * 1024 * 1024)
            .await
            .unwrap(),
    ));
    let partition = Mbr::format_disk_with_signature_and_partition(
        source,
        XILA_DISK_SIGNATURE,
        PartitionKind::Xila,
    )
    .unwrap();
    let fs = little_fs::FileSystem::new_format(Box::leak(Box::new(partition)), 256).unwrap();
    fs.create_file(Path::from_str("/seed-proof")).unwrap();
    let mut context = Context::new_empty();
    fs.lookup_file(
        &mut context,
        Path::from_str("/seed-proof"),
        AccessFlags::Write.into(),
    )
    .unwrap();
    fs.write_sync(&mut context, b"from-build", 0).unwrap();
    fs.close(&mut context).unwrap();
    drop(fs);
    let mut bytes = vec![0; 4 * 1024 * 1024];
    DirectBaseOperations::read_sync(source, &mut bytes, 0).unwrap();
    source.discard_unsaved().unwrap();
    remove(&source_name).await;
    let array = js_sys::Uint8Array::from(bytes.as_slice());
    let blob = web_sys::Blob::new_with_u8_array_sequence(&js_sys::Array::of1(&array)).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let name = name();
    // The disk manager creates the same zero-byte placeholder before switching.
    drop(
        DriveDevice::new(Path::from_str(&name), bytes.len())
            .await
            .unwrap(),
    );
    let drive = Box::leak(Box::new(
        DriveDevice::new_seeded(Path::from_str(&name), bytes.len(), &url)
            .await
            .unwrap(),
    ));
    assert!(drive.is_new());
    let fs = storage::mount(drive).unwrap();
    let mut context = Context::new_empty();
    fs.lookup_file(
        &mut context,
        Path::from_str("/seed-proof"),
        AccessFlags::Read.into(),
    )
    .unwrap();
    let mut proof = [0; 10];
    fs.read_sync(&mut context, &mut proof, 0).unwrap();
    assert_eq!(&proof, b"from-build");
    fs.close(&mut context).unwrap();
    drop(fs);
    drive.sync().await.unwrap();
    drive.unmount().unwrap();
    web_sys::Url::revoke_object_url(&url).unwrap();
    let reopened = DriveDevice::new_seeded(
        Path::from_str(&name),
        bytes.len(),
        "invalid-protocol://no-fetch",
    )
    .await
    .unwrap();
    assert!(!reopened.is_new());
    drop(reopened);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn malformed_seed_is_not_committed_and_can_be_retried() {
    let array = js_sys::Uint8Array::new_with_length(4 * 1024 * 1024);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&js_sys::Array::of1(&array)).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let name = name();
    let drive = Box::leak(Box::new(
        DriveDevice::new_seeded(Path::from_str(&name), array.length() as usize, &url)
            .await
            .unwrap(),
    ));
    assert!(storage::mount(drive).is_err());
    drive.discard_unsaved().unwrap();
    web_sys::Url::revoke_object_url(&url).unwrap();
    let reopened = DriveDevice::new(Path::from_str(&name), array.length() as usize)
        .await
        .unwrap();
    assert!(reopened.is_new());
    drop(reopened);
    remove(&name).await;
}
