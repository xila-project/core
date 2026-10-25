#![cfg(target_arch = "wasm32")]

// The driver's graphics dependency exports LVGL's platform hooks, which link
// against the same ABI implementations used by the browser example.
use abi_definitions as _;
use drivers_wasm::devices::DriveDevice;
use file_system::{
    DirectBaseOperations, DirectBlockDevice, Error, MountOperations, Path, Position,
};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::FileSystemDirectoryHandle;

wasm_bindgen_test_configure!(run_in_browser);
drivers_wasm::memory::instantiate_global_allocator!();

#[path = "../../../examples/wasm/src/storage.rs"]
mod storage;

#[path = "opfs/bootstrap.rs"]
mod bootstrap;

fn name() -> String {
    format!("xila-opfs-test-{}.img", js_sys::Math::random())
}

async fn remove(name: &str) {
    let directory = JsFuture::from(
        web_sys::window()
            .unwrap()
            .navigator()
            .storage()
            .get_directory(),
    )
    .await
    .unwrap()
    .dyn_into::<FileSystemDirectoryHandle>()
    .unwrap();
    JsFuture::from(directory.remove_entry(name)).await.unwrap();
}

#[wasm_bindgen_test]
async fn saves_and_reopens_without_erasing_existing_data() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 4096).await.unwrap();
    assert_eq!(drive.get_block_size(), Ok(512));
    assert_eq!(drive.get_block_count(), Ok(8));
    assert_eq!(drive.write_sync(b"persistent", 513), Ok(10));
    drive.sync().await.unwrap();
    drop(drive);

    let drive = DriveDevice::new(Path::from_str(&name), 4096).await.unwrap();
    let mut buffer = [0; 10];
    assert_eq!(drive.read_sync(&mut buffer, 513), Ok(10));
    assert_eq!(&buffer, b"persistent");
    assert_eq!(drive.read_sync(&mut buffer, 0), Ok(10));
    assert_eq!(buffer, [0; 10]);
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn bounds_and_seeking_are_checked() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    assert_eq!(drive.write_sync(b"abcd", 510), Ok(2));
    let mut buffer = [0xff; 4];
    assert_eq!(drive.read_sync(&mut buffer, 510), Ok(2));
    assert_eq!(buffer, [b'a', b'b', 0xff, 0xff]);
    assert_eq!(drive.read_sync(&mut buffer, 512), Ok(0));
    assert_eq!(drive.write_sync(b"x", 512), Ok(0));
    assert_eq!(
        drive.read_sync(&mut buffer, u64::MAX),
        Err(Error::InvalidParameter)
    );
    assert_eq!(drive.write_sync(b"x", 513), Err(Error::InvalidParameter));
    assert_eq!(drive.set_position(10, &Position::Current(-5)), Ok(5));
    assert_eq!(drive.set_position(0, &Position::End(-1)), Ok(511));
    assert_eq!(
        drive.set_position(0, &Position::Start(513)),
        Err(Error::InvalidParameter)
    );
    assert_eq!(
        drive.set_position(0, &Position::Current(-1)),
        Err(Error::InvalidParameter)
    );
    drive.sync().await.unwrap();
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn invalid_geometry_and_names_are_rejected() {
    for size in [0, 1, 513, usize::MAX] {
        assert!(matches!(
            DriveDevice::new(Path::new("invalid.img"), size).await,
            Err(Error::InvalidParameter)
        ));
    }
    for name in ["", ".", "..", "nested/disk.img"] {
        assert!(matches!(
            DriveDevice::new(Path::from_str(name), 512).await,
            Err(Error::InvalidPath)
        ));
    }
}

#[wasm_bindgen_test]
async fn opening_with_a_different_size_does_not_truncate_the_disk() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drive.write_sync(b"keep", 0).unwrap();
    drive.sync().await.unwrap();
    drop(drive);
    assert!(matches!(
        DriveDevice::new(Path::from_str(&name), 1024).await,
        Err(Error::InvalidParameter)
    ));
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    let mut buffer = [0; 4];
    drive.read_sync(&mut buffer, 0).unwrap();
    assert_eq!(&buffer, b"keep");
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn simultaneous_writers_are_rejected_and_unmount_releases_the_lock() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    assert!(matches!(
        DriveDevice::new(Path::from_str(&name), 512).await,
        Err(Error::RessourceBusy)
    ));
    drive.write_sync(b"dirty", 0).unwrap();
    assert_eq!(drive.unmount(), Err(Error::RessourceBusy));
    drive.sync().await.unwrap();
    drive.unmount().unwrap();
    assert_eq!(drive.read_sync(&mut [0; 1], 0), Err(Error::NotMounted));
    assert_eq!(drive.sync().await, Err(Error::NotMounted));
    let other = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drop(other);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn stops_writes_before_sync_and_unmount() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drive.write_sync(b"saved", 0).unwrap();

    drive.stop_writes();

    assert_eq!(drive.write_sync(b"lost", 0), Err(Error::RessourceBusy));
    drive.sync().await.unwrap();
    drive.unmount().unwrap();

    let reopened = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    let mut contents = [0; 5];
    reopened.read_sync(&mut contents, 0).unwrap();
    assert_eq!(&contents, b"saved");
    reopened.unmount().unwrap();
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn writes_during_a_save_are_saved_by_the_next_sync() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drive.write_sync(b"old", 0).unwrap();
    let started = pause_next_save();
    let mut save = std::pin::pin!(drive.sync());
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(save.as_mut().poll(&mut context), Poll::Pending));
    let release = JsFuture::from(started)
        .await
        .unwrap()
        .dyn_into::<js_sys::Function>()
        .unwrap();
    drive.write_sync(b"new", 0).unwrap();
    release.call0(&JsValue::NULL).unwrap();
    save.await.unwrap();
    assert_eq!(drive.unmount(), Err(Error::RessourceBusy));
    drive.sync().await.unwrap();
    drive.unmount().unwrap();
    let reopened = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    let mut buffer = [0; 3];
    reopened.read_sync(&mut buffer, 0).unwrap();
    assert_eq!(&buffer, b"new");
    drop(reopened);
    remove(&name).await;
}

#[wasm_bindgen(inline_js = "
export function failNextSave() {
    const prototype = FileSystemFileHandle.prototype;
    const original = prototype.createWritable;
    prototype.createWritable = function(...args) {
        prototype.createWritable = original;
        return Promise.reject(new DOMException('Test quota exhaustion', 'QuotaExceededError'));
    };
}
export function pauseNextSave() {
    return new Promise(started => {
        const prototype = FileSystemFileHandle.prototype;
        const original = prototype.createWritable;
        prototype.createWritable = async function(...args) {
            prototype.createWritable = original;
            const stream = await original.apply(this, args);
            await new Promise(release => started(release));
            return stream;
        };
    });
}
let failedStream;
export function failNextSaveStage(stage) {
    const prototype = FileSystemFileHandle.prototype;
    const original = prototype.createWritable;
    prototype.createWritable = async function(...args) {
        prototype.createWritable = original;
        failedStream = await original.apply(this, args);
        failedStream[stage] = () => Promise.reject(new DOMException('Test storage failure', 'QuotaExceededError'));
        return failedStream;
    };
}
export async function failedStreamAcceptsWrites() {
    try {
        await FileSystemWritableFileStream.prototype.write.call(failedStream, new Uint8Array([1]));
        return true;
    } catch (_) {
        return false;
    }
}
")]
extern "C" {
    #[wasm_bindgen(js_name = failNextSave)]
    fn fail_next_save();
    #[wasm_bindgen(js_name = pauseNextSave)]
    fn pause_next_save() -> js_sys::Promise;
    #[wasm_bindgen(js_name = failNextSaveStage)]
    fn fail_next_save_stage(stage: &str);
    #[wasm_bindgen(js_name = failedStreamAcceptsWrites)]
    fn failed_stream_accepts_writes() -> js_sys::Promise;
}

async fn failed_stream_save_preserves_disk_and_retries(stage: &str) {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drive.write_sync(b"old", 0).unwrap();
    drive.sync().await.unwrap();
    drive.write_sync(b"new", 0).unwrap();
    fail_next_save_stage(stage);
    assert_eq!(drive.sync().await, Err(Error::NoSpaceLeft));
    assert_eq!(drive.unmount(), Err(Error::RessourceBusy));
    // Exercise the real stream after failure: an aborted stream rejects I/O.
    assert_eq!(
        JsFuture::from(failed_stream_accepts_writes())
            .await
            .unwrap(),
        JsValue::FALSE
    );
    let directory = JsFuture::from(
        web_sys::window()
            .unwrap()
            .navigator()
            .storage()
            .get_directory(),
    )
    .await
    .unwrap()
    .dyn_into::<FileSystemDirectoryHandle>()
    .unwrap();
    let handle = JsFuture::from(directory.get_file_handle(&name))
        .await
        .unwrap()
        .dyn_into::<web_sys::FileSystemFileHandle>()
        .unwrap();
    let file = JsFuture::from(handle.get_file())
        .await
        .unwrap()
        .dyn_into::<web_sys::File>()
        .unwrap();
    let contents = JsFuture::from(file.array_buffer()).await.unwrap();
    assert_eq!(
        js_sys::Uint8Array::new(&contents).subarray(0, 3).to_vec(),
        b"old"
    );
    drive.sync().await.unwrap();
    drop(drive);
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    let mut buffer = [0; 3];
    drive.read_sync(&mut buffer, 0).unwrap();
    assert_eq!(&buffer, b"new");
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn failed_write_aborts_the_stream_and_preserves_the_disk() {
    failed_stream_save_preserves_disk_and_retries("write").await;
}

#[wasm_bindgen_test]
async fn failed_commit_aborts_the_stream_and_preserves_the_disk() {
    failed_stream_save_preserves_disk_and_retries("close").await;
}

#[wasm_bindgen_test]
async fn failed_save_keeps_the_committed_disk_and_can_be_retried() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drive.write_sync(b"old", 0).unwrap();
    drive.sync().await.unwrap();
    drive.write_sync(b"new", 0).unwrap();
    fail_next_save();
    assert_eq!(drive.sync().await, Err(Error::NoSpaceLeft));
    assert_eq!(drive.unmount(), Err(Error::RessourceBusy));
    let directory = JsFuture::from(
        web_sys::window()
            .unwrap()
            .navigator()
            .storage()
            .get_directory(),
    )
    .await
    .unwrap()
    .dyn_into::<FileSystemDirectoryHandle>()
    .unwrap();
    let handle = JsFuture::from(directory.get_file_handle(&name))
        .await
        .unwrap()
        .dyn_into::<web_sys::FileSystemFileHandle>()
        .unwrap();
    let file = JsFuture::from(handle.get_file())
        .await
        .unwrap()
        .dyn_into::<web_sys::File>()
        .unwrap();
    let contents = JsFuture::from(file.array_buffer()).await.unwrap();
    assert_eq!(
        js_sys::Uint8Array::new(&contents).subarray(0, 3).to_vec(),
        b"old"
    );
    drive.sync().await.unwrap();
    drop(drive);
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    let mut buffer = [0; 3];
    drive.read_sync(&mut buffer, 0).unwrap();
    assert_eq!(&buffer, b"new");
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn cancelled_save_waiters_do_not_allow_an_old_snapshot_to_overwrite_new_data() {
    use std::{
        future::Future,
        task::{Context, Waker},
    };
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    drive.write_sync(b"old", 0).unwrap();
    let started = pause_next_save();
    let mut save = Box::pin(drive.sync());
    assert!(
        save.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    let release = JsFuture::from(started)
        .await
        .unwrap()
        .dyn_into::<js_sys::Function>()
        .unwrap();
    assert_eq!(drive.unmount(), Err(Error::RessourceBusy));
    drop(save);
    drive.write_sync(b"new", 0).unwrap();
    release.call0(&JsValue::NULL).unwrap();
    drive.sync().await.unwrap();
    drop(drive);
    let drive = DriveDevice::new(Path::from_str(&name), 512).await.unwrap();
    let mut buffer = [0; 3];
    drive.read_sync(&mut buffer, 0).unwrap();
    assert_eq!(&buffer, b"new");
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn littlefs_file_and_mbr_survive_reopening_the_opfs_disk() {
    use file_system::{
        AccessFlags, BaseOperations, Context, FileSystemOperations, Flags, XILA_DISK_SIGNATURE,
        mbr::{Mbr, PartitionKind},
    };
    let name = name();
    // Match the example's static devices. Unmount releases the disk-sized
    // cache and browser lock; only the small wrappers live for the test process.
    let drive = Box::leak(Box::new(
        DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
            .await
            .unwrap(),
    ));
    let partition = Box::leak(Box::new(
        Mbr::find_or_create_partition_with_signature(
            drive,
            XILA_DISK_SIGNATURE,
            PartitionKind::Xila,
        )
        .unwrap(),
    ));
    let file_system = little_fs::FileSystem::get_or_format(partition, 256).unwrap();
    file_system.create_file(Path::new("/saved.txt")).unwrap();
    let mut context = Context::new_empty();
    file_system
        .lookup_file(
            &mut context,
            Path::new("/saved.txt"),
            Flags::from(AccessFlags::Write),
        )
        .unwrap();
    file_system
        .write_sync(&mut context, b"saved across reload", 0)
        .unwrap();
    file_system.close(&mut context).unwrap();
    drop(file_system);
    drive.sync().await.unwrap();
    drive.unmount().unwrap();

    let drive = Box::leak(Box::new(
        DriveDevice::new(Path::from_str(&name), 4 * 1024 * 1024)
            .await
            .unwrap(),
    ));
    assert_eq!(
        Mbr::read_from_device(drive).unwrap().get_disk_signature(),
        XILA_DISK_SIGNATURE
    );
    let partition = Box::leak(Box::new(
        Mbr::find_or_create_partition_with_signature(
            drive,
            XILA_DISK_SIGNATURE,
            PartitionKind::Xila,
        )
        .unwrap(),
    ));
    // Mount without formatting: a broken persistence implementation must fail.
    let file_system = little_fs::FileSystem::new(partition, 256).unwrap();
    let mut context = Context::new_empty();
    file_system
        .lookup_file(
            &mut context,
            Path::new("/saved.txt"),
            Flags::from(AccessFlags::Read),
        )
        .unwrap();
    let mut buffer = [0; 19];
    assert_eq!(file_system.read_sync(&mut context, &mut buffer, 0), Ok(19));
    assert_eq!(&buffer, b"saved across reload");
    file_system.close(&mut context).unwrap();
    drop(file_system);
    drive.sync().await.unwrap();
    drive.unmount().unwrap();
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn existing_disk_never_requires_a_seed_fetch() {
    let name = name();
    let drive = DriveDevice::new(Path::from_str(&name), 4096).await.unwrap();
    drive.write_sync(b"saved", 512).unwrap();
    drive.sync().await.unwrap();
    drop(drive);
    let drive = DriveDevice::new_seeded(
        Path::from_str(&name),
        4096,
        "invalid-protocol://must-not-fetch",
    )
    .await
    .unwrap();
    let mut bytes = [0; 5];
    drive.read_sync(&mut bytes, 512).unwrap();
    assert_eq!(&bytes, b"saved");
    drop(drive);
    remove(&name).await;
}

#[wasm_bindgen_test]
async fn failed_seed_download_keeps_an_empty_disk_retryable() {
    let name = name();
    assert!(
        DriveDevice::new_seeded(Path::from_str(&name), 4096, "invalid-protocol://offline")
            .await
            .is_err()
    );
    let drive = DriveDevice::new(Path::from_str(&name), 4096).await.unwrap();
    assert!(drive.is_new());
    drop(drive);
    remove(&name).await;
}
