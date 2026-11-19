#![cfg(all(feature = "native-dev", target_os = "linux", target_arch = "x86_64"))]

use core::{
    alloc::Layout,
    ptr::NonNull,
    task::{Context, Poll, Waker},
};
use portable_atomic::{AtomicBool, Ordering};
use std::{process::Command, sync::Mutex};

use executable::{Error, ExecutableContext, NativeFormat, Standard, execute};
use file_system::{
    AccessFlags, BaseOperations, CharacterDevice, Context as DeviceContext, CreateFlags, Flags,
    MountOperations, Permissions, Position, Size, Special,
};
use native_loader::{Error as LoaderError, ExecutableMemory, LoadedImage, Region};
use virtual_file_system::File;

mod common;

use common::{FixtureEntropy, MAPPINGS, Mmap};

testing_support::instantiate_global_allocator!();

// SAFETY: this test installs only the fixture it compiled locally, with a stack
// sized for the debug ABI call chain and on the test's single executor.
static LOADER: NativeFormat = unsafe { NativeFormat::new(&Mmap, 64 * 1024, 32 * 1024, 32 * 1024) };

#[derive(Default)]
struct ReadState {
    ready: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

struct PendingDevice(&'static ReadState);
impl BaseOperations for PendingDevice {
    fn read(
        &self,
        _: &mut DeviceContext,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        if self.0.ready.load(Ordering::Relaxed) {
            buffer.copy_from_slice(b"ok");
            return Poll::Ready(Ok(2));
        }
        assert!(
            self.0
                .waker
                .lock()
                .unwrap()
                .replace(cx.waker().clone())
                .is_none()
        );
        Poll::Pending
    }

    fn write(
        &self,
        _: &mut DeviceContext,
        _: &mut Context<'_>,
        _: &[u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::Error::UnsupportedOperation))
    }

    fn clone_context(&self, _: &DeviceContext) -> file_system::Result<DeviceContext> {
        Ok(DeviceContext::new_empty())
    }
}
impl MountOperations for PendingDevice {}
impl CharacterDevice for PendingDevice {}

fn build_fixture(name: &str, flags: &str) -> Vec<u8> {
    let output = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let recipe = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../native_loader/fixtures/build.sh"
    );
    assert!(
        Command::new("sh")
            .arg(recipe)
            .arg(&output)
            .env("XILA_FIXTURE_FLAGS", flags)
            .status()
            .unwrap()
            .success()
    );
    std::fs::read(output).unwrap()
}

/// Memory backend that fails the test if the loader allocates.
struct NoMemory;

// SAFETY: it never hands out memory.
unsafe impl ExecutableMemory for NoMemory {
    fn allocate(&self, _: Layout) -> Result<NonNull<u8>, LoaderError> {
        panic!("the loader allocated for a rejected image");
    }
    unsafe fn finish_writes(
        &self,
        _: NonNull<u8>,
        _: Layout,
        _: &mut dyn Iterator<Item = Region>,
    ) -> Result<(), LoaderError> {
        unreachable!()
    }
    unsafe fn free(&self, _: NonNull<u8>, _: Layout) {
        unreachable!()
    }
}

#[test]
fn rejects_c_images_with_constructors_before_allocating() {
    let bytes = build_fixture("xila-constructor.elf", "-DXILA_FIXTURE_CONSTRUCTOR");
    assert!(matches!(
        LoadedImage::load(&bytes, &NoMemory, 32 * 1024),
        Err(LoaderError::Constructors)
    ));
}

#[task::test]
async fn executes_c_elf_from_vfs_with_host_services() {
    let bytes = build_fixture("xila-start.elf", "");

    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let tasks = task::test_manager();
    let users = Box::leak(Box::new(users::Manager::new()));
    let parent = tasks.get_current_task_identifier().await;
    let device = file_system::MemoryDevice::<512>::new_static(128 * 1024);
    let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(128)).unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
    ));
    let mut context = ExecutableContext::new(tasks, users, vfs, time);
    context.formats = const { &[LOADER.registration()] };
    let context = Box::leak(Box::new(context));

    let read_state = Box::leak(Box::new(ReadState::default()));
    vfs.mount_character_device(parent, "/pending", PendingDevice(read_state))
        .await
        .unwrap();
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let mut program = File::open(vfs, parent, "/program", create).await.unwrap();
    assert_eq!(program.write(&bytes).await.unwrap(), bytes.len());
    program.close(vfs).await.unwrap();

    let mut output = File::open(vfs, parent, "/output", create).await.unwrap();
    let make_standard = async || {
        Standard::new(
            context,
            File::open(vfs, parent, "/pending", AccessFlags::Read.into())
                .await
                .unwrap(),
            output.duplicate().await.unwrap(),
            output.duplicate().await.unwrap(),
        )
    };

    vfs.set_permissions(
        parent,
        "/program",
        Permissions::EXECUTABLE.set_special(Special::NONE.set_set_user_identifier(true)),
    )
    .await
    .unwrap();
    assert!(matches!(
        execute(context, "/program", vec![], make_standard().await, None).await,
        Err(Error::PermissionDenied)
    ));
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    vfs.set_permissions(parent, "/program", Permissions::EXECUTABLE)
        .await
        .unwrap();

    let without_entropy = execute(context, "/program", vec![], make_standard().await, None)
        .await
        .unwrap();
    assert_eq!(
        without_entropy.join().await,
        73,
        "ELF without an entropy device"
    );

    vfs.create_directory(parent, &"/devices").await.unwrap();
    vfs.create_directory(parent, &"/devices/random")
        .await
        .unwrap();
    vfs.mount_character_device(parent, "/devices/random/source", FixtureEntropy)
        .await
        .unwrap();
    let reader = execute(
        context,
        "/program",
        vec!["entropy".into()],
        make_standard().await,
        None,
    )
    .await
    .unwrap();
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 1);

    // Wait for the actual ABI read to register a waker. This task is on the
    // same executor as the C program and can run only after it yields.
    let waker = loop {
        if reader.is_finished() {
            panic!("the ELF returned {} before suspending", reader.join().await);
        }
        if let Some(waker) = read_state.waker.lock().unwrap().take() {
            break waker;
        }
        task::yield_now().await;
    };
    read_state.ready.store(true, Ordering::Relaxed);
    waker.wake();

    assert_eq!(reader.join().await, 73);
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);

    // Replacing the source path with a file must never supply entropy.
    vfs.unmount("/devices/random/source", false).await.unwrap();
    let mut replacement = File::open(vfs, parent, "/devices/random/source", create)
        .await
        .unwrap();
    assert_eq!(replacement.write(&[0xa5; 8]).await.unwrap(), 8);
    replacement.close(vfs).await.unwrap();
    let with_replacement = execute(context, "/program", vec![], make_standard().await, None)
        .await
        .unwrap();
    assert_eq!(
        with_replacement.join().await,
        73,
        "ELF with a regular file in place of entropy"
    );

    output.set_position(&Position::Start(0)).await.unwrap();
    let mut received = [0; 2];
    assert_eq!(output.read(&mut received).await.unwrap(), 2);
    assert_eq!(&received, b"ok");

    // Reject a changed ABI before allocating.
    let mut malformed = bytes;
    let note = malformed
        .windows(5)
        .position(|bytes| bytes == b"XILA\0")
        .unwrap();
    malformed[note + 8] ^= 0x80;
    assert!(matches!(
        LoadedImage::load(&malformed, &Mmap, 32 * 1024),
        Err(LoaderError::IncompatibleAbi(_))
    ));
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
}
