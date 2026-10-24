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
fn headless_debug_application_answers_through_the_mounted_transport() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{fs::MetadataExt, net::UnixStream},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    struct Runtime(Child);
    impl Drop for Runtime {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let fixture = Fixture::new();
    let log_path = fixture.0.join("runtime.log");
    let log = fs::File::create(&log_path).unwrap();
    let mut runtime = Runtime(
        Command::new(env!("CARGO_BIN_EXE_native_example"))
            .args([
                "--display-driver",
                "headless",
                "--resolution",
                "320x240",
                "--no-network",
            ])
            .arg("--disk-size")
            .arg(SIZE.to_string())
            .arg("--disk-path")
            .arg(fixture.0.join("drive.img"))
            .arg("--seed-image")
            .arg(fixture.0.join("seed.img"))
            .env("TMPDIR", &fixture.0)
            .env_remove("XILA_UI_CONTROL_ENDPOINT")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let uid = fs::metadata(&fixture.0).unwrap().uid();
    let endpoint = fixture.0.join(format!("xila-debug-{uid}/control.sock"));
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut stream = loop {
        if let Ok(stream) = UnixStream::connect(&endpoint) {
            break stream;
        }
        assert!(
            runtime.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "Headless Debug transport did not start: {}",
            fs::read_to_string(&log_path).unwrap(),
        );
        thread::sleep(Duration::from_millis(20));
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(b"{\"op\":\"resolution\"}\n").unwrap();
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).unwrap();
    assert!(response.contains(r#""ok":true"#), "{response}");
    assert!(response.contains(r#""width":320"#), "{response}");
    assert!(response.contains(r#""height":240"#), "{response}");
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
    assert!(error.to_string().contains("mise run prepare-native-image"));
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
