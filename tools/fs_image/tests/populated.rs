use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "xila-seed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("root/binaries")).unwrap();
        fs::write(path.join("root/binaries/example"), vec![42; 18000]).unwrap();
        Self(path)
    }
    fn build(&self, name: &str) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_fs_image"))
            .arg(self.0.join(name))
            .args(["4194304", "--root"])
            .arg(self.0.join("root"))
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn populated_images_are_verified_and_reproducible() {
    let f = Fixture::new();
    for name in ["one.img", "two.img"] {
        let output = f.build(name);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Verified"));
    }
    assert_eq!(
        fs::read(f.0.join("one.img")).unwrap(),
        fs::read(f.0.join("two.img")).unwrap()
    );
    let before = fs::read(f.0.join("one.img")).unwrap();
    assert!(!f.build("one.img").status.success());
    assert_eq!(before, fs::read(f.0.join("one.img")).unwrap());
}
#[cfg(unix)]
#[test]
fn rejects_symlinks_without_leaving_an_image() {
    let f = Fixture::new();
    std::os::unix::fs::symlink("/etc/passwd", f.0.join("root/link")).unwrap();
    assert!(!f.build("bad.img").status.success());
    assert!(!f.0.join("bad.img").exists());
}
#[test]
fn out_of_space_removes_incomplete_output() {
    let f = Fixture::new();
    fs::write(f.0.join("root/binaries/huge"), vec![1; 5 * 1024 * 1024]).unwrap();
    assert!(!f.build("full.img").status.success());
    assert!(!f.0.join("full.img").exists());
}
