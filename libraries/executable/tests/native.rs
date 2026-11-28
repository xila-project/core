#![cfg(all(feature = "native-dev", target_os = "linux", target_arch = "x86_64"))]

use core::task::{Context, Poll, Waker};
use portable_atomic::{AtomicBool, AtomicUsize, Ordering};
use std::{process::Command, sync::Mutex};

use abi_application::ProcessLaunch;
use executable::{Error, ExecutableContext, Home, NativeFormat, Standard, execute, execute_with};
use file_system::{
    AccessFlags, BaseOperations, CharacterDevice, Context as DeviceContext, CreateFlags,
    DirectBaseOperations, DirectCharacterDevice, Flags, MountOperations, Permissions, Position,
    Size, Special,
};
use native_loader::{Error as LoaderError, LoadedImage, Startup};
use stackful::Stack;
use virtual_file_system::{File, VirtualFileSystem};

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
        Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
    }

    fn clone_context(&self, _: &DeviceContext) -> file_system::Result<DeviceContext> {
        Ok(DeviceContext::new_empty())
    }
}
impl MountOperations for PendingDevice {}
impl CharacterDevice for PendingDevice {}

/// A sink that records its writes, to prove that finished processes released their streams.
static SINK: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct Sink;
impl DirectBaseOperations for Sink {
    fn read(&self, _: &mut Context<'_>, _: &mut [u8], _: Size) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
    }

    fn write(
        &self,
        _: &mut Context<'_>,
        buffer: &[u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        SINK.lock().unwrap().extend_from_slice(buffer);
        Poll::Ready(Ok(buffer.len()))
    }
}
impl MountOperations for Sink {
    fn unmount(&self) -> file_system::Result<()> {
        Ok(())
    }
}
impl DirectCharacterDevice for Sink {}

fn build_fixture(name: &str, flags: &str) -> Vec<u8> {
    build_fixture_from("start.c", name, flags)
}

/// Builds a fixture into a path unique to this call, then reads it back: nextest runs
/// tests in parallel processes and several tests build the same fixture name, so a
/// fixed path would let one test read the ELF while another process rewrites it.
fn build_fixture_from(source: &str, name: &str, flags: &str) -> Vec<u8> {
    static BUILD_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let output = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "{name}.{}.{}",
        std::process::id(),
        BUILD_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let recipe = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../native_loader/fixtures/build.sh"
    );
    assert!(
        Command::new("sh")
            .arg(recipe)
            .arg(&output)
            .env("XILA_FIXTURE_FLAGS", flags)
            .env("XILA_FIXTURE_SOURCE", source)
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&output).unwrap();
    std::fs::remove_file(&output).unwrap();
    bytes
}

/// Compiler flags that link with lld. `-fuse-ld=lld` looks for `ld.lld` on `PATH`;
/// distributions often install it only in a versioned LLVM directory, which clang
/// knows, so that directory is added as a program search path (`-B`, which gcc and
/// clang both accept).
fn lld_flags() -> String {
    let on_path = std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|directory| directory.join("ld.lld").is_file())
    });
    if on_path {
        return "-fuse-ld=lld".into();
    }
    let candidate = Command::new("clang")
        .arg("-print-prog-name=ld.lld")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file());
    let Some(directory) = candidate.as_deref().and_then(std::path::Path::parent) else {
        panic!("ld.lld not found on PATH or next to clang; install lld (LLVM's linker)");
    };
    format!("-fuse-ld=lld -B{}", directory.display())
}

/// The section names of a little-endian ELF64 image.
fn section_names(bytes: &[u8]) -> Vec<String> {
    let word = |offset: usize, size: usize| {
        let mut value = [0; 8];
        value[..size].copy_from_slice(&bytes[offset..offset + size]);
        u64::from_le_bytes(value) as usize
    };
    assert_eq!(
        &bytes[..6],
        b"\x7fELF\x02\x01",
        "a little-endian ELF64 image"
    );
    let (table, entry_size, count, names) =
        (word(0x28, 8), word(0x3a, 2), word(0x3c, 2), word(0x3e, 2));
    let header = |index: usize| table + index * entry_size;
    let strings = word(header(names) + 0x18, 8);
    (0..count)
        .map(|index| {
            let start = strings + word(header(index), 4);
            let end = start + bytes[start..].iter().position(|&byte| byte == 0).unwrap();
            String::from_utf8_lossy(&bytes[start..end]).into_owned()
        })
        .collect()
}

#[test]
fn rejects_lifecycle_arrays_without_entry_opt_in() {
    let bytes = build_fixture("xila-constructor.elf", "-DXILA_FIXTURE_CONSTRUCTOR");
    assert!(matches!(
        LoadedImage::load(&bytes, &Mmap, 32 * 1024),
        Err(LoaderError::MissingLifecycleNote)
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

    // Reject an image built for another Xila minor version before allocating.
    let mut malformed = bytes;
    let note = malformed
        .windows(5)
        .position(|bytes| bytes == b"XILA\0")
        .unwrap();
    // The descriptor follows the padded name: its low byte is the low byte of the minor version.
    malformed[note + 8] ^= 0x80;
    let running = native_loader::Version::new(
        abi_application::XILA_VERSION_MAJOR as u16,
        abi_application::XILA_VERSION_MINOR as u16,
    );
    let declared = native_loader::Version::new(running.major, running.minor ^ 0x80);
    assert_eq!(
        LoadedImage::load(&malformed, &Mmap, 32 * 1024).err(),
        Some(LoaderError::IncompatibleXilaVersion { declared, running })
    );
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);

    // lld omits the empty relocation section in this fixture. Its dynamic
    // metadata must stay outside the ABI note segment.
    let bytes = build_fixture(
        "xila-no-relocations.elf",
        &format!("{} -DXILA_FIXTURE_NO_RELOCATIONS", lld_flags()),
    );
    let sections = section_names(&bytes);
    assert!(
        !sections
            .iter()
            .any(|name| name == ".rel.dyn" || name == ".rela.dyn"),
        "the fixture no longer covers an image without relocations: {sections:?}"
    );
    drop(LoadedImage::load(&bytes, &Mmap, 32 * 1024).unwrap());
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);

    // A note the linker script does not name must not join the ABI note segment.
    let bytes = build_fixture(
        "xila-custom-note.elf",
        &format!("{} -DXILA_FIXTURE_CUSTOM_NOTE", lld_flags()),
    );
    assert!(
        section_names(&bytes)
            .iter()
            .any(|name| name == ".note.other")
    );
    drop(LoadedImage::load(&bytes, &Mmap, 32 * 1024).unwrap());
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
}

/// A native program spawns copies of itself through `xila_process_spawn`, which goes
/// through `executable::execute`, waits for them and collects their statuses.
#[task::test]
async fn native_process_spawns_waits_and_exits_through_the_host() {
    let bytes = build_fixture_from("process.c", "xila-process.elf", "");
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

    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let write = async |path: &str, contents: &[u8], permissions: Permissions| {
        let mut file = File::open(vfs, parent, path, create).await.unwrap();
        assert_eq!(file.write(contents).await.unwrap(), contents.len());
        file.close(vfs).await.unwrap();
        vfs.set_permissions(parent, path, permissions)
            .await
            .unwrap();
    };
    write("/process.elf", &bytes, Permissions::EXECUTABLE).await;
    write("/noexec.elf", &bytes, Permissions::NONE).await;
    write("/bad.elf", b"\x7fELFnot an image", Permissions::EXECUTABLE).await;
    write("/stdin", b"", Permissions::ALL_READ_WRITE).await;
    vfs.mount_character_device(parent, "/sink", Sink)
        .await
        .unwrap();
    let sink = File::open(vfs, parent, "/sink", AccessFlags::Write.into())
        .await
        .unwrap();
    // The parent's task is the program's parent: its environment is copied at launch.
    tasks
        .set_environment_variable(parent, "FROM_HOST", "host")
        .await
        .unwrap();
    let tasks_before = tasks.get_task_count().await;

    let standard = Standard::new(
        context,
        File::open(vfs, parent, "/stdin", AccessFlags::Read.into())
            .await
            .unwrap(),
        sink.duplicate().await.unwrap(),
        sink.duplicate().await.unwrap(),
    );
    let launch = executable::Launch::default().with_home(Home {
        path: file_system::PathOwned::new("/".into()).unwrap(),
        rights: 3,
    });
    let handle = execute_with(
        context,
        "/process.elf",
        vec!["parent".into(), parent.to_string()],
        standard,
        None,
        launch,
    )
    .await
    .unwrap();
    // A nonzero status below 73 is the line of the failed check in process.c.
    assert_eq!(handle.join().await, 73);

    // Every image mapping and task is gone, and the exiting child's output was written.
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    assert_eq!(tasks.get_task_count().await, tasks_before);
    assert_eq!(SINK.lock().unwrap().as_slice(), b"bye");
    // Each process, including the one that exited from a nested frame, closed its
    // duplicates of the sink: only this handle keeps the device busy.
    assert!(vfs.unmount("/sink", false).await.is_err());
    sink.close(vfs).await.unwrap();
    vfs.unmount("/sink", false).await.unwrap();
    // The explicit environment of a child never touched the parent's task.
    assert_eq!(
        tasks
            .get_environment_variable(parent, "FROM_HOST")
            .await
            .unwrap()
            .get_value(),
        "host"
    );
}

/// A program that never starts must still release its arguments and startup streams
/// (#775): the host state sits in a `ManuallyDrop` the moment it is built, so only the
/// allocation's owner can drop it when the coroutine entry never ran. Here the spawn
/// future is dropped before its first poll, as when a caller gives up or a spawn is
/// refused. Exhausting the allocator at the allocation itself cannot be injected: that
/// path drops the same state, and is checked by inspection only.
#[test]
fn dropped_spawn_future_releases_the_startup_arguments() {
    // The other tests assert exact `MAPPINGS` counts; hold the same lock as
    // `#[task::test]` so this test's mapping never overlaps them.
    let _test_lock = task::test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let manager: &'static task::Manager = Box::leak(Box::new(task::Manager::new()));
    let bytes = build_fixture("xila-unstarted.elf", "");
    let before = testing_support::MemoryManager::thread_allocations();
    let image = LoadedImage::load(&bytes, &Mmap, 32 * 1024).expect("load");
    let stack = stackful::Stack::new(16 * 1024);
    let arguments = vec![
        String::from_utf8(vec![b'a'; 4000]).unwrap(),
        String::from("second"),
    ];
    let spawned = unsafe {
        image.spawn(
            manager,
            task::Manager::ROOT_TASK_IDENTIFIER,
            "unstarted",
            stack,
            native_loader::Startup {
                arguments,
                ..Default::default()
            },
            |status| status,
        )
    }
    .expect("the process state allocates");
    // Never polled: the task never starts, and the future owns everything.
    drop(spawned);
    let net = testing_support::MemoryManager::thread_allocations()
        .since(before)
        .net_bytes();
    assert_eq!(net, 0, "dropping an unpolled spawn leaked {net} bytes");
}

/// A character device whose reads never finish, and that can be unmounted. It is not
/// zero-sized: the file system tells devices apart by address.
struct Blocked(#[allow(dead_code)] u8);
impl BaseOperations for Blocked {
    fn read(
        &self,
        _: &mut DeviceContext,
        _: &mut Context<'_>,
        _: &mut [u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Pending
    }

    fn write(
        &self,
        _: &mut DeviceContext,
        _: &mut Context<'_>,
        _: &[u8],
        _: Size,
    ) -> Poll<file_system::Result<usize>> {
        Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
    }

    fn clone_context(&self, _: &DeviceContext) -> file_system::Result<DeviceContext> {
        Ok(DeviceContext::new_empty())
    }
}
impl MountOperations for Blocked {
    fn unmount(&self) -> file_system::Result<()> {
        Ok(())
    }
}
impl CharacterDevice for Blocked {}

/// A file system with a blocked device at `/blocked`, the context of a process running on
/// it, and a handle the test keeps on the device, to tell a closed handle from a leaked one
/// and from one closed twice: the device stays in use until the test closes its own.
async fn blocked_world() -> (
    &'static task::Manager,
    task::TaskIdentifier,
    &'static VirtualFileSystem,
    &'static ExecutableContext,
    File<'static>,
) {
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
    let context = Box::leak(Box::new(ExecutableContext::new(tasks, users, vfs, time)));
    vfs.mount_character_device(parent, "/blocked", Blocked(0))
        .await
        .unwrap();
    let kept = File::open(vfs, parent, "/blocked", AccessFlags::Read.into())
        .await
        .unwrap();
    (tasks, parent, vfs, context, kept)
}

/// The startup of a process whose standard input is a handle on the blocked device.
async fn blocked_startup(
    vfs: &'static VirtualFileSystem,
    parent: task::TaskIdentifier,
    context: &'static ExecutableContext,
) -> Startup {
    let input = File::open(vfs, parent, "/blocked", AccessFlags::Read.into())
        .await
        .unwrap();
    Startup {
        arguments: vec!["blocked".into(), "x".repeat(3000)],
        standard_input: Some(abi_application::File::new(input.into_synchronous_file())),
        process: Some(ProcessLaunch {
            host: context,
            environment: None,
            home: None,
            network: 0,
        }),
        ..Default::default()
    }
}

/// The device is in use while the test's handle is open, and only then.
async fn only_the_test_holds(vfs: &'static VirtualFileSystem, kept: File<'static>) {
    assert!(
        vfs.unmount("/blocked", false).await.is_err(),
        "the test's own handle keeps the device in use: the program closed its own twice"
    );
    kept.close(vfs).await.unwrap();
    vfs.unmount("/blocked", false)
        .await
        .expect("the program's handle was closed");
}

/// A native program cancelled while it is suspended in a read: its task future is dropped,
/// the coroutine's frames are discarded, and the host closes its startup streams, frees its
/// stack, heap and state, and unmaps its image (#731).
#[task::test]
async fn cancelled_native_program_releases_its_streams_image_and_state() {
    let bytes = build_fixture_from("cancel.c", "xila-cancel.elf", "");
    let (_, parent, vfs, context, kept) = blocked_world().await;
    let before = testing_support::MemoryManager::thread_allocations();
    let startup = blocked_startup(vfs, parent, context).await;
    let image = LoadedImage::load(&bytes, &Mmap, 32 * 1024).unwrap();
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 1);
    let mut program = Box::pin(unsafe { image.start(Stack::new(64 * 1024), startup) }.unwrap());
    // The program blocks reading its standard input.
    let polled = core::future::poll_fn(|cx| Poll::Ready(program.as_mut().poll(cx))).await;
    assert!(polled.is_pending());
    assert!(vfs.unmount("/blocked", false).await.is_err());

    drop(program);
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0, "the image is unmapped");
    let net = testing_support::MemoryManager::thread_allocations()
        .since(before)
        .net_bytes();
    assert_eq!(net, 0, "stack, heap, state and arguments are all freed");
    only_the_test_holds(vfs, kept).await;
}

/// The same program cancelled before it ran: nothing started, and the streams are still
/// released by the owner of the allocation.
#[task::test]
async fn native_program_cancelled_before_its_first_poll_releases_its_streams() {
    let bytes = build_fixture_from("cancel.c", "xila-cancel.elf", "");
    let (_, parent, vfs, context, kept) = blocked_world().await;
    let startup = blocked_startup(vfs, parent, context).await;
    let image = LoadedImage::load(&bytes, &Mmap, 32 * 1024).unwrap();
    let program = unsafe { image.start(Stack::new(64 * 1024), startup) }.unwrap();
    assert!(vfs.unmount("/blocked", false).await.is_err());
    drop(program);
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    only_the_test_holds(vfs, kept).await;
}

/// The task manager refuses the spawn: the spawn future resolves to an error, and the
/// streams and the image it took are released with it.
#[task::test]
async fn refused_spawn_releases_the_startup_streams_and_the_image() {
    let bytes = build_fixture_from("cancel.c", "xila-cancel.elf", "");
    let (tasks, parent, vfs, context, kept) = blocked_world().await;
    let startup = blocked_startup(vfs, parent, context).await;
    let image = LoadedImage::load(&bytes, &Mmap, 32 * 1024).unwrap();
    let unknown_parent = task::TaskIdentifier::new(task::TaskIdentifier::MAXIMUM);
    assert_ne!(unknown_parent, parent);
    let spawned = unsafe {
        image.spawn(
            tasks,
            unknown_parent,
            "refused",
            Stack::new(64 * 1024),
            startup,
            |status| status,
        )
    }
    .unwrap();
    assert!(spawned.await.is_err(), "the parent is not a task");
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    only_the_test_holds(vfs, kept).await;
}
