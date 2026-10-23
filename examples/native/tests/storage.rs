#![cfg(target_os = "linux")]
#[path = "../src/storage.rs"]
mod storage;
use abi_definitions as _;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use xila::file_system::{
    DirectBaseOperations, FileSystemOperations, XILA_DISK_SIGNATURE,
    mbr::{Mbr, PartitionKind},
};
drivers_std::instantiate_global_allocator!();
static NEXT: AtomicU64 = AtomicU64::new(0);
const SIZE: u64 = 4 * 1024 * 1024;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "xila-native-seed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let name = path.join("seed.img");
        let name = name.to_str().unwrap();
        let drive = drivers_std::drive_file::FileDriveDevice::new_static(&name, SIZE);
        let partition = Mbr::format_disk_with_signature_and_partition(
            drive,
            XILA_DISK_SIGNATURE,
            PartitionKind::Xila,
        )
        .unwrap();
        let fs =
            xila::little_fs::FileSystem::new_format(Box::leak(Box::new(partition)), 256).unwrap();
        drop(fs);
        drive.flush_sync().unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn imports_once_and_preserves_existing_disk_without_seed() {
    let f = Fixture::new();
    let disk = f.0.join("drive.img");
    let seed = f.0.join("seed.img");
    assert!(storage::prepare_disk(&disk, &seed, SIZE).unwrap());
    assert_eq!(fs::read(&disk).unwrap(), fs::read(&seed).unwrap());
    let drive = Box::leak(Box::new(
        drivers_std::drive_file::FileDriveDevice::open_existing(&disk).unwrap(),
    ));
    let filesystem = xila::little_fs::mount_disk(drive, 256).unwrap();
    filesystem
        .create_file(xila::file_system::Path::from_str("/user-kept-file"))
        .unwrap();
    drop(filesystem);
    drive.flush_sync().unwrap();
    fs::remove_file(&seed).unwrap();
    let before = fs::read(&disk).unwrap();
    assert!(!storage::prepare_disk(&disk, &seed, SIZE).unwrap());
    assert_eq!(fs::read(&disk).unwrap(), before);
    assert!(!storage::prepare_disk(&disk, &disk, SIZE).unwrap());
    let filesystem = xila::little_fs::mount_disk(drive, 256).unwrap();
    let mut attributes =
        xila::file_system::Attributes::new().set_mask(xila::file_system::AttributeFlags::all());
    filesystem
        .get_attributes(
            xila::file_system::Path::from_str("/user-kept-file"),
            &mut attributes,
        )
        .unwrap();
    assert_eq!(attributes.get_kind(), Some(&xila::file_system::Kind::File));
}
#[test]
fn existing_invalid_or_empty_files_are_never_replaced_or_resized() {
    let f = Fixture::new();
    let disk = f.0.join("drive.img");
    let seed = f.0.join("seed.img");
    for bytes in [b"".as_slice(), b"keep corrupt disk".as_slice()] {
        fs::write(&disk, bytes).unwrap();
        assert!(storage::prepare_disk(&disk, &seed, SIZE).is_err());
        assert_eq!(fs::read(&disk).unwrap(), bytes);
    }
}
#[test]
fn invalid_seed_leaves_no_destination_or_temporary_file() {
    let f = Fixture::new();
    let disk = f.0.join("drive.img");
    let seed = f.0.join("seed.img");
    fs::write(&seed, vec![0; SIZE as usize]).unwrap();
    assert!(storage::prepare_disk(&disk, &seed, SIZE).is_err());
    assert!(!disk.exists());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}

#[test]
fn missing_seed_explains_how_to_prepare_the_native_image() {
    let f = Fixture::new();
    let disk = f.0.join("drive.img");
    let seed = f.0.join("missing-seed.img");

    let error = storage::prepare_disk(&disk, &seed, SIZE).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(
        error
            .to_string()
            .contains("cargo make prepare-native-image")
    );
    assert!(error.to_string().contains(seed.to_str().unwrap()));
    assert!(!disk.exists());
}

#[test]
fn matching_size_corrupt_disk_is_preserved_on_mount_failure() {
    let f = Fixture::new();
    let disk = f.0.join("drive.img");
    let bytes = vec![0x42; SIZE as usize];
    fs::write(&disk, &bytes).unwrap();
    assert!(!storage::prepare_disk(&disk, &f.0.join("missing.img"), SIZE).unwrap());
    let drive = Box::leak(Box::new(
        drivers_std::drive_file::FileDriveDevice::open_existing(&disk).unwrap(),
    ));
    assert!(xila::little_fs::mount_disk(drive, 256).is_err());
    assert_eq!(fs::read(&disk).unwrap(), bytes);
}
