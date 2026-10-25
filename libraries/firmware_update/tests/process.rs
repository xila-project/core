#![cfg(feature = "native")]
mod support;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use support::*;
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(dir: &Path, args: &[&str], success: bool) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_xila-firmware-sim"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "args={args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn fixture() -> Fixture {
    let dir = std::env::temp_dir().join(format!(
        "xila-fw-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("key"), key().verifying_key().to_bytes()).unwrap();
    fs::write(dir.join("old"), b"old firmware").unwrap();
    fs::write(dir.join("old.manifest"), manifest(b"old firmware", 1)).unwrap();
    fs::write(dir.join("new"), [0x5a; 137]).unwrap();
    fs::write(dir.join("new.manifest"), manifest(&[0x5a; 137], 2)).unwrap();
    run(
        &dir,
        &[
            "init",
            "--flash",
            "flash.img",
            "--key",
            "key",
            "--target",
            "xila-native-test",
            "--manifest",
            "old.manifest",
            "--image",
            "old",
        ],
        true,
    );
    Fixture(dir)
}
fn update(dir: &Path, extra: &[&str], success: bool) {
    let mut args = vec![
        "update",
        "--flash",
        "flash.img",
        "--key",
        "key",
        "--target",
        "xila-native-test",
        "--manifest",
        "new.manifest",
        "--image",
        "new",
    ];
    args.extend_from_slice(extra);
    run(dir, &args, success);
}
fn boot(dir: &Path, confirm: bool) -> String {
    let mut args = vec![
        "boot",
        "--flash",
        "flash.img",
        "--key",
        "key",
        "--target",
        "xila-native-test",
    ];
    if confirm {
        args.push("--confirm");
    }
    run(dir, &args, true)
}
#[test]
fn child_process_stream_trial_confirm_and_revert() {
    let f = fixture();
    assert!(boot(&f.0, false).contains("slot=0 trial=false"));
    update(&f.0, &[], true);
    assert!(boot(&f.0, false).contains("slot=1 trial=true"));
    assert!(boot(&f.0, false).contains("slot=0 trial=false"));
    update(&f.0, &[], true);
    assert!(boot(&f.0, true).contains("slot=1 trial=true"));
    assert!(boot(&f.0, false).contains("slot=1 trial=false"));
    let status = run(
        &f.0,
        &[
            "inspect",
            "--flash",
            "flash.img",
            "--key",
            "key",
            "--target",
            "xila-native-test",
        ],
        true,
    );
    assert!(status.contains("security_floor=2"));
}
#[test]
fn child_process_close_does_not_activate_and_fault_is_persistent() {
    let f = fixture();
    update(&f.0, &["--stage-only"], true);
    assert!(boot(&f.0, false).contains("slot=0 trial=false"));
    update(
        &f.0,
        &["--fault-operation", "20", "--fault-byte", "31"],
        false,
    );
    assert!(boot(&f.0, false).contains("slot=0 trial=false"));
    let mut corrupted = fs::read(f.0.join("new.manifest")).unwrap();
    corrupted[80] ^= 1;
    fs::write(f.0.join("new.manifest"), corrupted).unwrap();
    update(&f.0, &[], false);
    assert!(boot(&f.0, false).contains("slot=0 trial=false"));
}
#[test]
fn file_medium_requires_exclusive_process_ownership() {
    use firmware_update::nor::FileNor;
    let f = fixture();
    let _locked = FileNor::open(&f.0.join("flash.img"), None).unwrap();
    assert!(FileNor::open(&f.0.join("flash.img"), None).is_err());
}
