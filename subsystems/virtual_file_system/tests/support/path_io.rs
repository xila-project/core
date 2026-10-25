use super::*;
use file_system::{BaseOperations, CharacterDevice, MountOperations, Size};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct ShortDevice {
    bytes: Arc<Mutex<Vec<u8>>>,
    closes: Arc<AtomicUsize>,
    stop: usize,
    fail: bool,
}
impl MountOperations for ShortDevice {}
impl CharacterDevice for ShortDevice {}
impl BaseOperations for ShortDevice {
    fn clone_context(&self, _: &Context) -> file_system::Result<Context> {
        Ok(Context::new_empty())
    }
    fn read(
        &self,
        _: &mut Context,
        _: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        position: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            if position as usize >= self.stop {
                return if self.fail {
                    Err(file_system::Error::InputOutput)
                } else {
                    Ok(0)
                };
            }
            let bytes = self.bytes.lock().unwrap();
            let count = buffer
                .len()
                .min(2)
                .min(bytes.len().saturating_sub(position as usize));
            buffer[..count].copy_from_slice(&bytes[position as usize..position as usize + count]);
            Ok(count)
        })
    }
    fn write(
        &self,
        _: &mut Context,
        _: &mut core::task::Context<'_>,
        buffer: &[u8],
        position: Size,
    ) -> core::task::Poll<file_system::Result<usize>> {
        file_system::ready(|| {
            if position as usize >= self.stop {
                return if self.fail {
                    Err(file_system::Error::InputOutput)
                } else {
                    Ok(0)
                };
            }
            let count = buffer.len().min(2);
            self.bytes
                .lock()
                .unwrap()
                .extend_from_slice(&buffer[..count]);
            Ok(count)
        })
    }
    fn close(&self, _: &mut Context) -> file_system::Result<()> {
        self.closes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
async fn setup(
    bytes: &[u8],
    stop: usize,
    fail: bool,
) -> (
    TaskIdentifier,
    &'static VirtualFileSystem,
    Arc<Mutex<Vec<u8>>>,
    Arc<AtomicUsize>,
) {
    let (task, vfs) = initialize().await;
    let bytes = Arc::new(Mutex::new(bytes.to_vec()));
    let closes = Arc::new(AtomicUsize::new(0));
    vfs.mount_character_device(
        task,
        "/short",
        ShortDevice {
            bytes: bytes.clone(),
            closes: closes.clone(),
            stop,
            fail,
        },
    )
    .await
    .unwrap();
    (task, vfs, bytes, closes)
}
#[task::test]
async fn path_read_fills_buffer_across_short_reads() {
    let (task, vfs, _, closes) = setup(b"abcdef", usize::MAX, false).await;
    let mut bytes = [0; 6];
    File::read_slice_from_path(vfs, task, "/short", &mut bytes)
        .await
        .unwrap();
    assert_eq!(&bytes, b"abcdef");
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}
#[task::test]
async fn path_write_retries_short_writes() {
    let (task, vfs, bytes, closes) = setup(b"", usize::MAX, false).await;
    File::write_to_path(vfs, task, "/short", b"abcdef")
        .await
        .unwrap();
    assert_eq!(&*bytes.lock().unwrap(), b"abcdef");
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}
#[task::test]
async fn path_helpers_report_incomplete_io_and_close_once() {
    for fail in [false, true] {
        let (task, vfs, _, closes) = setup(b"abcdef", 2, fail).await;
        let mut bytes = [0; 6];
        assert!(
            File::read_slice_from_path(vfs, task, "/short", &mut bytes)
                .await
                .is_err()
        );
        assert_eq!(closes.load(Ordering::SeqCst), 1);
        assert!(
            File::write_to_path(vfs, task, "/short", b"abcdef")
                .await
                .is_err()
        );
        assert_eq!(closes.load(Ordering::SeqCst), 2);
    }
}
