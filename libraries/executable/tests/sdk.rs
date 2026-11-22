//! Builds the SDK's hello example and a probe program with the SDK's CMake toolchain, then
//! runs the resulting static PIEs through the host path: the VFS, the executable dispatch
//! table, the native loader, the ABI table and the C library with its POSIX layer on top.
//!
//! The SDK comes from `mise run sdk:build`, which `test:native-loader` runs first. It lives
//! in `target/sdk/<target>/xila-sdk-<xila_version>-<target>` under the workspace; set
//! `XILA_SDK_DIRECTORY` to test another build.
#![cfg(all(feature = "native-dev", target_os = "linux", target_arch = "x86_64"))]

mod common;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

use common::{FixtureEntropy, MAPPINGS, Mmap};
use core::sync::atomic::Ordering;
use executable::{ExecutableContext, Home, Launch, NativeFormat, Standard, execute, execute_with};
use file_system::{AccessFlags, CreateFlags, Flags, PathOwned, Permissions};
use task::TaskIdentifier;
use virtual_file_system::{File, VirtualFileSystem};

testing_support::instantiate_global_allocator!();

const XILA_RIGHT_READ: u8 = 1;
const XILA_RIGHT_WRITE: u8 = 2;

// SAFETY: the test runs the SDK example it just compiled, on the test's single executor,
// with a stack sized for the debug build of the ABI call chain.
static LOADER: NativeFormat =
    unsafe { NativeFormat::new(&Mmap, 64 * 1024, 256 * 1024, 256 * 1024) };

fn run(command: &mut Command) {
    let status = command.status().unwrap();
    assert!(status.success(), "{command:?} failed with {status}");
}

fn sdk_directory() -> PathBuf {
    if let Ok(directory) = std::env::var("XILA_SDK_DIRECTORY") {
        return directory.into();
    }
    let target = "x86_64-unknown-linux-gnu";
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sdk")
        .join(target);
    let mut found: Vec<_> = std::fs::read_dir(&parent)
        .unwrap_or_else(|_| panic!("no SDK in {}: run `mise run sdk:build`", parent.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("xila-sdk-") && name.ends_with(target))
        })
        .collect();
    found.sort();
    found
        .pop()
        .unwrap_or_else(|| panic!("no SDK in {}: run `mise run sdk:build`", parent.display()))
}

/// Configure and build a CMake project that produces `<name>.elf` with the SDK's toolchain
/// file.
fn build_sdk_program(source: &Path, name: &str, format: &str) -> Vec<u8> {
    let sdk = sdk_directory();
    // CMake caches its flags and does not track the SDK's libraries as dependencies: key
    // the build directory by the toolchain file and every library, so a rebuilt SDK never
    // reuses a stale configuration or a stale link.
    let mut hasher = DefaultHasher::new();
    for file in [
        "cmake/xila-toolchain.cmake",
        "lib/crt0.o",
        "lib/libxila.a",
        "lib/libxila_posix.a",
        "lib/libc.a",
    ] {
        std::fs::read(sdk.join(file)).unwrap().hash(&mut hasher);
    }
    format.hash(&mut hasher);
    name.hash(&mut hasher);
    let build = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sdk-{name}-{:016x}", hasher.finish()));
    run(Command::new("cmake")
        .arg("-S")
        .arg(source)
        .arg("-B")
        .arg(&build)
        .arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            sdk.join("cmake/xila-toolchain.cmake").display()
        ))
        .arg(format!("-DXILA_PRINTF_FORMAT={format}"))
        .arg("-DCMAKE_BUILD_TYPE=MinSizeRel"));
    run(Command::new("cmake").arg("--build").arg(&build));
    std::fs::read(build.join(format!("{name}.elf"))).unwrap()
}

fn build_sdk_example(format: &str) -> Vec<u8> {
    build_sdk_program(
        &sdk_directory().join("share/xila/example/hello"),
        "hello",
        format,
    )
}

/// The POSIX probes are test programs of this repository, not part of the SDK's examples.
fn build_posix_probe(name: &str) -> Vec<u8> {
    build_sdk_program(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../abi/application/sdk/test/probe"),
        name,
        "integer",
    )
}

/// The stack protector test program: built with -fstack-protector-strong.
fn build_ssp() -> Vec<u8> {
    build_sdk_program(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../abi/application/sdk/test/ssp"),
        "ssp",
        "integer",
    )
}

async fn read_all(vfs: &'static VirtualFileSystem, parent: TaskIdentifier, path: &str) -> Vec<u8> {
    let mut file = File::open(vfs, parent, path, AccessFlags::Read.into())
        .await
        .unwrap();
    let mut text = Vec::new();
    let mut chunk = [0; 256];
    loop {
        let count = file.read(&mut chunk).await.unwrap();
        if count == 0 {
            break;
        }
        text.extend_from_slice(&chunk[..count]);
    }
    file.close(vfs).await.unwrap();
    text
}

async fn install(
    vfs: &'static VirtualFileSystem,
    parent: TaskIdentifier,
    path: &str,
    bytes: &[u8],
) {
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let mut program = File::open(vfs, parent, path, create).await.unwrap();
    assert_eq!(program.write(bytes).await.unwrap(), bytes.len());
    program.close(vfs).await.unwrap();
    vfs.set_permissions(parent, path, Permissions::EXECUTABLE)
        .await
        .unwrap();
}

/// Start the program at `path` with `arguments`; stdout and stderr go to their own files.
async fn start(
    context: &'static ExecutableContext,
    parent: TaskIdentifier,
    path: &str,
    label: &str,
    arguments: &[&str],
) -> task::JoinHandle<isize> {
    let vfs = context.virtual_file_system;
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    let (stdout, stderr) = (format!("/{label}.out"), format!("/{label}.err"));
    let input = File::open(vfs, parent, "/input", create).await.unwrap();
    let output = File::open(vfs, parent, stdout.as_str(), create)
        .await
        .unwrap();
    let error = File::open(vfs, parent, stderr.as_str(), create)
        .await
        .unwrap();
    // Every launch carries its own home: the root of the filesystem, read and write.
    let launch = Launch::default().with_home(Home {
        path: PathOwned::new("/".into()).unwrap(),
        rights: XILA_RIGHT_READ | XILA_RIGHT_WRITE,
    });
    execute_with(
        context,
        path,
        arguments
            .iter()
            .map(|argument| (*argument).into())
            .collect(),
        Standard::new(context, input, output, error),
        None,
        launch,
    )
    .await
    .unwrap()
}

/// Run the program at `path` with `arguments` to its end. Returns the exit status, stdout and
/// stderr.
async fn launch(
    context: &'static ExecutableContext,
    parent: TaskIdentifier,
    path: &str,
    label: &str,
    arguments: &[&str],
) -> (isize, Vec<u8>, Vec<u8>) {
    let vfs = context.virtual_file_system;
    let handle = start(context, parent, path, label, arguments).await;
    let status = handle.join().await;
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    (
        status,
        read_all(vfs, parent, &format!("/{label}.out")).await,
        read_all(vfs, parent, &format!("/{label}.err")).await,
    )
}

async fn setup() -> (&'static ExecutableContext, TaskIdentifier) {
    setup_with(|_| ()).await
}

/// A context with the SDK's loader and a filesystem, which `configure` may extend.
async fn setup_with(
    configure: impl FnOnce(&mut ExecutableContext),
) -> (&'static ExecutableContext, TaskIdentifier) {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let tasks = task::test_manager();
    let users = Box::leak(Box::new(users::Manager::new()));
    let parent = tasks.get_current_task_identifier().await;
    // This host-only filesystem holds all six SDK ELFs and their output files.
    let device = file_system::MemoryDevice::<512>::new_static(320 * 1024);
    let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
    ));
    let mut context = ExecutableContext::new(tasks, users, vfs, time);
    context.formats = const { &[LOADER.registration()] };
    configure(&mut context);
    let context = Box::leak(Box::new(context));

    (context, parent)
}

#[task::test]
#[ignore = "run with mise run sdk:rust-example"]
async fn runs_sdk_rust_hello() {
    let path = std::env::var_os("XILA_RUST_HELLO").expect("set by sdk:rust-example");
    let bytes = std::fs::read(path).unwrap();
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    install(vfs, parent, "/rust-hello", &bytes).await;
    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);

    for (arguments, status, stdout, stderr) in [
        (
            vec![],
            73,
            b"Hello from Rust on Xila!\n".as_slice(),
            b"".as_slice(),
        ),
        (
            vec!["panic".into()],
            134,
            b"".as_slice(),
            b"panicked\n".as_slice(),
        ),
    ] {
        let output_path = format!("/rust-{status}.out");
        let error_path = format!("/rust-{status}.err");
        let input = File::open(vfs, parent, "/input", create).await.unwrap();
        let output = File::open(vfs, parent, output_path.as_str(), create)
            .await
            .unwrap();
        let error = File::open(vfs, parent, error_path.as_str(), create)
            .await
            .unwrap();
        let handle = execute(
            context,
            "/rust-hello",
            arguments,
            Standard::new(context, input, output, error),
            None,
        )
        .await
        .unwrap();
        assert_eq!(handle.join().await, status);
        assert_eq!(read_all(vfs, parent, &output_path).await, stdout);
        assert_eq!(read_all(vfs, parent, &error_path).await, stderr);
        assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    }
}

#[task::test]
async fn runs_sdk_c_program_through_the_posix_layer() {
    let (context, parent) = setup().await;
    let vfs = context.virtual_file_system;
    vfs.create_directory(parent, &"/devices").await.unwrap();
    vfs.create_directory(parent, &"/devices/random")
        .await
        .unwrap();
    vfs.mount_character_device(parent, "/devices/random/source", FixtureEntropy)
        .await
        .unwrap();

    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);
    File::open(vfs, parent, "/input", create)
        .await
        .unwrap()
        .close(vfs)
        .await
        .unwrap();

    context
        .task_manager
        .set_environment_variable(parent, "SDK_GREETING", "hello from the environment")
        .await
        .unwrap();

    // Integer-only printf (the default) and the double variant, each a separate build.
    for format in ["integer", "double"] {
        let program_path = format!("/program-{format}");
        install(vfs, parent, &program_path, &build_sdk_example(format)).await;
        // The program exits with the status given as its first argument.
        let (status, stdout, stderr) = launch(context, parent, &program_path, format, &["3"]).await;
        assert_eq!(status, 3, "{}", String::from_utf8_lossy(&stderr));
        let text = String::from_utf8(stdout).unwrap();
        println!("{text}");
        assert!(text.contains("hello from Xila, 1 argument(s)\n"), "{text}");
        assert!(
            text.contains("greeting: hello from the environment\n"),
            "{text}"
        );
        assert!(text.contains("entropy: a5a5a5a5\n"), "{text}");
        assert!(text.ends_with("hello: ok\n"), "{text}");

        // The program removed everything it created, except the stream it never closed:
        // exit flushed that one.
        assert!(vfs.get_statistics(&"/sdk-hello").await.is_err());
        assert_eq!(
            read_all(vfs, parent, "/sdk-exit.txt").await,
            b"flushed at exit\n"
        );
    }

    install(vfs, parent, "/probe", &build_posix_probe("probe")).await;
    context
        .task_manager
        .set_environment_variable(parent, "SDK_PROBE", "inherited")
        .await
        .unwrap();
    context
        .task_manager
        .set_environment_variable(parent, "SDK_OTHER", "a=b")
        .await
        .unwrap();
    let (status, stdout, stderr) =
        launch(context, parent, "/probe", "environment", &["environment"]).await;
    assert_eq!(status, 0, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(stdout, b"environment: ok\n");

    install(vfs, parent, "/no-environ", &build_posix_probe("no-environ")).await;
    let (status, stdout, stderr) = launch(context, parent, "/no-environ", "no-environ", &[]).await;
    assert_eq!(status, 0);
    assert!(stdout.is_empty() && stderr.is_empty());

    // _exit flushes nothing: the buffered writes are lost, the file the program created
    // stays empty, the unbuffered stderr write got through.
    let (status, stdout, stderr) = launch(
        context,
        parent,
        "/probe",
        "underscore",
        &["underscore-exit"],
    )
    .await;
    assert_eq!(status, 7);
    assert_eq!(stderr, b"seen");
    assert_eq!(stdout, b"", "stdout was flushed by _exit");
    assert_eq!(read_all(vfs, parent, "/probe-underscore.txt").await, b"");

    // fflush(NULL) does not make exit forget the streams: everything written afterwards,
    // an atexit handler's too, is kept, and stdout's unterminated tail is flushed.
    let (status, stdout, stderr) =
        launch(context, parent, "/probe", "flush", &["flush-then-exit"]).await;
    assert_eq!(status, 8, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(stdout, b"tail");
    assert_eq!(
        read_all(vfs, parent, "/probe-flush.txt").await,
        b"one\ntwo\nhandler\n"
    );
    assert_eq!(
        read_all(vfs, parent, "/probe-flush-other.txt").await,
        b"g1g2"
    );

    // The root directory, directory entries and errno values.
    let (status, stdout, stderr) = launch(context, parent, "/probe", "root", &["root"]).await;
    assert_eq!(status, 0, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(stdout, b"probe: ok\n");

    let (status, stdout, stderr) =
        launch(context, parent, "/probe", "descriptors", &["descriptors"]).await;
    assert_eq!(status, 0, "SDK descriptor probe failed: {stderr:?}");
    assert_eq!(stdout, b"descriptors: ok\n");

    // posix_spawn, posix_spawnp, file actions, waitpid and wait, on the probe itself.
    let (status, stdout, stderr) = launch(context, parent, "/probe", "spawn", &["spawn"]).await;
    assert_eq!(
        status,
        0,
        "SDK spawn probe failed: {}",
        String::from_utf8_lossy(&stderr)
    );
    // The two children that were not redirected share the parent's standard output.
    assert_eq!(
        stdout,
        b"child: argc=4 argv1=spawn-child argv2=5 inherited=from-parent explicit=(unset)\n\
          child: argc=3 argv1=spawn-child argv2=6 inherited=from-parent explicit=(unset)\n\
          spawn: ok\n"
    );
    assert_eq!(
        read_all(vfs, parent, "/spawn-inherit.txt").await,
        b"child: argc=3 argv1=spawn-child argv2=3 inherited=from-parent explicit=(unset)\n"
    );
    assert_eq!(
        read_all(vfs, parent, "/spawn-explicit.txt").await,
        b"child: argc=3 argv1=spawn-child argv2=4 inherited=(unset) explicit=from-array\n"
    );

    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../abi/application/sdk/test/threads");
    let threads = build_sdk_program(&source, "threads", "integer");
    install(vfs, parent, "/threads", &threads).await;
    let (status, stdout, stderr) = launch(context, parent, "/threads", "threads", &[]).await;
    assert_eq!(
        status, 0,
        "failed check at threads.c:{status}, stderr={stderr:?}"
    );
    assert_eq!(stdout, b"threads: ok\n");
    let (status, stdout, stderr) = launch(
        context,
        parent,
        "/threads",
        "print-contention",
        &["print-contention"],
    )
    .await;
    assert_eq!(status, 0, "printing threads: {stderr:?}");
    assert_eq!(stdout, b"worker\nmain\n".repeat(4));
    // pthread_exit is unsupported on main: it aborts before the TLS destructors run.
    let (status, stdout, stderr) = launch(
        context,
        parent,
        "/threads",
        "main-pthread-exit",
        &["main-pthread-exit"],
    )
    .await;
    assert_eq!(status, 134, "main pthread_exit: {stderr:?}");
    assert_eq!(stdout, b"", "main pthread_exit ran a destructor");
    vfs.create_named_pipe(&"/thread-pipe", 8, parent)
        .await
        .unwrap();
    let mut reader = File::open(vfs, parent, "/thread-pipe", AccessFlags::Read.into())
        .await
        .unwrap();
    for (mode, expected, output) in [
        ("main-return", 29, "partial line"),
        ("locked-stdout", 29, ""),
        ("worker-exit", 31, ""),
        ("worker-exit-flush", 32, "partial line"),
    ] {
        let (status, stdout, stderr) = launch(context, parent, "/threads", mode, &[mode]).await;
        assert_eq!(status, expected, "threads {mode}, stderr={stderr:?}");
        let mut buf = [0];
        // A synchronous read returns WouldBlock if the leaked writer still holds the pipe open.
        let reader = reader.as_synchronous_file_mut();
        assert_eq!(reader.read(&mut buf), Ok(1), "threads {mode}");
        assert_eq!(&buf, b"x");
        assert_eq!(reader.read(&mut buf), Ok(0), "threads {mode}: expected EOF");
        assert_eq!(stdout, output.as_bytes(), "threads {mode}");
    }
    reader.close(vfs).await.unwrap();
    assert_eq!(
        read_all(vfs, parent, "/thread-buffered.txt").await,
        b"buffered file"
    );

    // A worker suspended in an open, holding the VFS namespace lock while it waits for the
    // device lock the host holds, when main calls exit(): exit stops the worker first,
    // which gives the namespace lock back, so closing and flushing the program's file, which
    // need it, finish while the host still holds the device lock.
    let handle = start(
        context,
        parent,
        "/threads",
        "exit-under-lock",
        &["exit-under-lock"],
    )
    .await;
    let mut join = core::pin::pin!(handle.join());
    let mut deadline = core::pin::pin!(task::sleep(core::time::Duration::from_secs(30)));
    let status = vfs
        .hold_character_device_lock(core::future::poll_fn(|cx| {
            if let core::task::Poll::Ready(status) = join.as_mut().poll(cx) {
                return core::task::Poll::Ready(Some(status));
            }
            deadline.as_mut().poll(cx).map(|()| None)
        }))
        .await;
    assert_eq!(
        status,
        Some(33),
        "exit under a held lock must end the process"
    );
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    assert_eq!(
        read_all(vfs, parent, "/thread-locked.txt").await,
        b"flushed"
    );

    // A program built with -fstack-protector-strong links against the SDK, and the guard
    // is seeded from the entropy device before main runs.
    install(vfs, parent, "/ssp", &build_ssp()).await;
    let (status, stdout, stderr) = launch(context, parent, "/ssp", "ssp-ok", &["ok"]).await;
    assert_eq!(status, 0, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(stdout, b"ssp: ok\n");

    // Overwriting the canary ends the process with status 134 and a message on stderr; the
    // host still gets its descriptors and mappings back (launch checks the mappings).
    let (status, stdout, stderr) = launch(context, parent, "/ssp", "ssp-smash", &["smash"]).await;
    assert_eq!(status, 134);
    assert_eq!(stdout, b"");
    assert_eq!(stderr, b"*** stack smashing detected ***\n");

    // A larger overflow also destroys the frames above main: the saved return addresses
    // and xila_main's jmp_buf, which _exit() would jump through (it crashes the host when
    // the handler uses it). The handler must leave through the process capability, whose
    // state (startup block, capability, standard streams) is host-owned and off the
    // coroutine stack (#766): sizes from 224 bytes to well past the 416 where it used to
    // crash the host still end with status 134. How far an overflow can go before it
    // leaves the coroutine stack depends on the build profile: in release 1024 bytes
    // already write into the next heap chunk (glibc then aborts the test with
    // "free(): invalid next size"), while debug holds up to 2 KiB. The largest size is
    // one both profiles hold; past it the program writes outside any memory the host gave it.
    for size in ["224", "320", "416", "512", "768"] {
        let (status, stdout, stderr) =
            launch(context, parent, "/ssp", "ssp-large", &["large", size]).await;
        assert_eq!(status, 134, "{size}");
        assert_eq!(stdout, b"", "{size}");
        assert_eq!(stderr, b"*** stack smashing detected ***\n", "{size}");
    }

    // The same overflow in a worker thread runs over the worker's frames and the host's
    // entry on the worker's coroutine stack. The worker's own state, which the handler's
    // xila_process_exit reads to find the process group, is in the worker's record on the
    // heap (#778): these sizes used to overwrite it and crash the host (from 160 bytes in
    // debug, 128 in release). A worker has few frames above the program's, so the overflow
    // soon reaches the top of its stack, where the coroutine switch keeps its link back to
    // the executor's stack, and crashes there instead: from 224 bytes in release, 768 in
    // debug. The largest size is one both profiles hold.
    for size in ["160", "192", "208"] {
        let (status, stdout, stderr) = launch(
            context,
            parent,
            "/ssp",
            "ssp-thread-large",
            &["thread-large", size],
        )
        .await;
        assert_eq!(status, 134, "thread {size}");
        assert_eq!(stdout, b"", "thread {size}");
        assert_eq!(
            stderr, b"*** stack smashing detected ***\n",
            "thread {size}"
        );
    }
}

/// A display the graphics manager draws on and nobody looks at.
#[cfg(feature = "graphics")]
mod screen {
    use core::task::{Context, Poll};
    use file_system::{
        ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectCharacterDevice,
        MountOperations, Size,
    };
    use graphics::{GET_RESOLUTION, Point, SET_DRAWING_AREA, WAS_RESIZED};
    use shared::AnyByLayout;

    pub struct Screen;

    impl MountOperations for Screen {}
    impl DirectCharacterDevice for Screen {}
    impl DirectBaseOperations for Screen {
        fn read(
            &self,
            _: &mut Context<'_>,
            _: &mut [u8],
            _: Size,
        ) -> Poll<file_system::Result<usize>> {
            Poll::Ready(Err(file_system::IoErrorKind::Unsupported))
        }

        fn write(
            &self,
            _: &mut Context<'_>,
            bytes: &[u8],
            _: Size,
        ) -> Poll<file_system::Result<usize>> {
            Poll::Ready(Ok(bytes.len()))
        }

        fn control(
            &self,
            _: &mut Context<'_>,
            command: ControlCommandIdentifier,
            _: &AnyByLayout,
            output: &mut AnyByLayout,
        ) -> Poll<file_system::Result<()>> {
            Poll::Ready(match command {
                GET_RESOLUTION::IDENTIFIER => GET_RESOLUTION::cast_output(output)
                    .map(|resolution| *resolution = Point::new(320, 240)),
                WAS_RESIZED::IDENTIFIER => {
                    WAS_RESIZED::cast_output(output).map(|resized| *resized = false)
                }
                SET_DRAWING_AREA::IDENTIFIER => Ok(()),
                _ => Err(file_system::IoErrorKind::Unsupported),
            })
        }
    }
}

/// Fails the test binary, rather than hanging it, when a test runs past its deadline: a
/// regression in how a process waits for the graphics lock stops the executor's only
/// thread, so the test itself could never notice. Dropping the guard disarms it.
#[cfg(feature = "graphics")]
struct Watchdog {
    _disarm: std::sync::mpsc::Sender<()>,
}

#[cfg(feature = "graphics")]
impl Watchdog {
    fn start(name: &'static str, seconds: u64) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                receiver.recv_timeout(core::time::Duration::from_secs(seconds))
            {
                // The harness captures `eprintln!`, and `exit` would discard it.
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr(),
                    "{name}: still running after {seconds} s: the graphics lock hangs"
                );
                std::process::exit(1);
            }
        });
        Self { _disarm: sender }
    }
}

/// A graphics manager on a screen nobody looks at, and a context that has it. The manager
/// is a process-wide singleton, so the graphics scenarios share one test and one setup.
#[cfg(feature = "graphics")]
async fn graphics_setup() -> (
    &'static graphics::Manager,
    &'static ExecutableContext,
    TaskIdentifier,
) {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let manager: &'static graphics::Manager = Box::leak(Box::new(
        graphics::initialize_with_input_devices(time, &screen::Screen, &[], 1024, false).await,
    ));
    let (context, parent) = setup_with(|context| context.graphics_manager = Some(manager)).await;
    let vfs = context.virtual_file_system;
    vfs.create_directory(parent, &"/devices").await.unwrap();
    vfs.create_directory(parent, &"/devices/random")
        .await
        .unwrap();
    vfs.mount_character_device(parent, "/devices/random/source", FixtureEntropy)
        .await
        .unwrap();
    (manager, context, parent)
}

/// A C program builds a window and a few objects through the SDK's graphics bindings, reads
/// properties back, polls the window's events until the test clicks its button and ends
/// with all of them alive: by returning, and by calling `exit`. Either way the host deletes
/// the objects, and a second process after it starts clean. Then a worker's `exit` while the
/// main thread waits for the graphics lock.
#[cfg(feature = "graphics")]
#[task::test]
async fn graphics_calls_reach_the_executor_and_a_process_leaves_no_objects() {
    use graphics::{call::State, lvgl};

    let _watchdog = Watchdog::start("graphics_calls_reach_the_executor", 180);
    let (manager, context, parent) = graphics_setup().await;
    let vfs = context.virtual_file_system;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../abi/application/sdk/test/graphics");
    install(
        vfs,
        parent,
        "/graphics",
        &build_sdk_program(&source, "graphics", "integer"),
    )
    .await;

    let screen = manager.get_current_screen().unwrap();
    let children = || {
        manager
            .lock_function_sync(|| Ok(unsafe { lvgl::lv_obj_get_child_count(screen) }))
            .unwrap()
    };
    let before = children();
    for (mode, status) in [("return", 0), ("exit", 7)] {
        let label = format!("graphics-{mode}");
        let handle = start(context, parent, "/graphics", &label, &[mode]).await;

        // The program creates /graphics-ready once its objects exist, then waits for a click.
        let mut waited = 0;
        while vfs.get_statistics(&"/graphics-ready").await.is_err() {
            assert!(
                waited < 5000 && !handle.is_finished(),
                "{mode}: the program did not become ready: {}",
                String::from_utf8_lossy(&read_all(vfs, parent, &format!("/{label}.err")).await)
            );
            task::sleep(core::time::Duration::from_millis(1)).await;
            waited += 1;
        }
        assert_eq!(manager.get_window_count().await.unwrap(), 1);
        assert_eq!(State::registered_state_count(manager), 1);
        assert_eq!(children(), before + 1);
        // Another task holds the graphics lock across an await. The program's next poll
        // waits for it by suspending: were it to block the executor's thread instead, this
        // task could never resume to release the lock.
        {
            let _lock = manager.lock().await;
            task::sleep(core::time::Duration::from_millis(20)).await;
        }
        // The window's first child is the program's button.
        let window = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
        manager
            .lock_function(|| {
                // SAFETY: the graphics lock is held and the window is alive.
                unsafe {
                    let button = lvgl::lv_obj_get_child(window, 0);
                    lvgl::lv_obj_send_event(
                        button,
                        lvgl::lv_event_code_t_LV_EVENT_CLICKED,
                        core::ptr::null_mut(),
                    );
                }
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(handle.join().await, status, "{mode}");
        assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
        assert_eq!(
            read_all(vfs, parent, &format!("/{label}.out")).await,
            b"graphics: ok\n",
            "{mode}: {}",
            String::from_utf8_lossy(&read_all(vfs, parent, &format!("/{label}.err")).await)
        );
        // The process ended with its objects alive: the host deleted them.
        assert_eq!(manager.get_window_count().await.unwrap(), 0, "{mode}");
        assert_eq!(State::registered_state_count(manager), 0, "{mode}");
        assert_eq!(children(), before, "{mode}");
    }

    exit_from_a_worker_waits_for_the_graphics_lock(manager, context, parent).await;
}

/// A worker calls `exit(9)` while the main thread is suspended in a graphics call, and the
/// test holds the graphics lock across both. The exit has to wait for the lock too, as the
/// main thread's call does, and then delete the objects: no window and no registered state
/// are left, and the process ends with status 9.
#[cfg(feature = "graphics")]
async fn exit_from_a_worker_waits_for_the_graphics_lock(
    manager: &'static graphics::Manager,
    context: &'static ExecutableContext,
    parent: TaskIdentifier,
) {
    use graphics::call::State;

    let vfs = context.virtual_file_system;
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../abi/application/sdk/test/exit_suspended");
    install(
        vfs,
        parent,
        "/exit-suspended",
        &build_sdk_program(&source, "exit_suspended", "integer"),
    )
    .await;

    let handle = start(context, parent, "/exit-suspended", "exit-suspended", &[]).await;
    let mut waited = 0;
    while vfs.get_statistics(&"/exit-ready").await.is_err() {
        assert!(
            waited < 5000 && !handle.is_finished(),
            "the program did not become ready: {}",
            String::from_utf8_lossy(&read_all(vfs, parent, "/exit-suspended.err").await)
        );
        task::sleep(core::time::Duration::from_millis(1)).await;
        waited += 1;
    }
    assert_eq!(manager.get_window_count().await.unwrap(), 1);
    assert_eq!(State::registered_state_count(manager), 1);

    {
        let _lock = manager.lock().await;
        // The main thread's next poll now waits for the lock, suspended.
        task::sleep(core::time::Duration::from_millis(20)).await;
        // The worker leaves: its exit waits for the same lock before it deletes anything.
        vfs.create_directory(parent, &"/go").await.unwrap();
        task::sleep(core::time::Duration::from_millis(100)).await;
        assert!(
            !handle.is_finished(),
            "exit did not wait for the graphics lock"
        );
    }

    assert_eq!(handle.join().await, 9);
    assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);
    assert_eq!(manager.get_window_count().await.unwrap(), 0);
    assert_eq!(State::registered_state_count(manager), 0);
}
