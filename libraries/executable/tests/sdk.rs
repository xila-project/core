//! Builds the SDK's hello example with the SDK's CMake toolchain, then runs the resulting
//! static PIE through the host path: the VFS, the executable dispatch table, the native
//! loader, the ABI table and the C library with its POSIX layer on top.
//!
//! The SDK comes from `mise run sdk:build`, which `test:native-loader` runs first. It lives
//! in `target/sdk/<target>/xila-sdk-<abi>-<target>` under the workspace; set
//! `XILA_SDK_DIRECTORY` to test another build.
#![cfg(all(feature = "native-dev", target_os = "linux", target_arch = "x86_64"))]

mod common;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::PathBuf,
    process::Command,
};

use common::{FixtureEntropy, MAPPINGS, Mmap};
use core::sync::atomic::Ordering;
use executable::{ExecutableContext, NativeFormat, Standard, execute};
use file_system::{AccessFlags, CreateFlags, Flags, Permissions, Position};
use virtual_file_system::File;

testing_support::instantiate_global_allocator!();

const XILA_RIGHT_READ: u32 = 1;
const XILA_RIGHT_WRITE: u32 = 2;

// SAFETY: the test runs the SDK example it just compiled, on the test's single executor,
// with a stack sized for the debug build of the ABI call chain.
static LOADER: NativeFormat = unsafe {
    NativeFormat::new(&Mmap, 64 * 1024, 256 * 1024, 256 * 1024)
        .with_home("/", XILA_RIGHT_READ | XILA_RIGHT_WRITE)
};

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

/// Configure and build `share/xila/example/hello` with the SDK's toolchain file.
fn build_sdk_example(format: &str) -> Vec<u8> {
    let sdk = sdk_directory();
    // CMake caches its flags: key the build directory by the toolchain file so a rebuilt
    // SDK never reuses a stale configuration.
    let toolchain = std::fs::read(sdk.join("cmake/xila-toolchain.cmake")).unwrap();
    let mut hasher = DefaultHasher::new();
    toolchain.hash(&mut hasher);
    format.hash(&mut hasher);
    let build = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sdk-hello-{:016x}", hasher.finish()));
    run(Command::new("cmake")
        .arg("-S")
        .arg(sdk.join("share/xila/example/hello"))
        .arg("-B")
        .arg(&build)
        .arg(format!(
            "-DCMAKE_TOOLCHAIN_FILE={}",
            sdk.join("cmake/xila-toolchain.cmake").display()
        ))
        .arg(format!("-DXILA_PRINTF_FORMAT={format}"))
        .arg("-DCMAKE_BUILD_TYPE=MinSizeRel"));
    run(Command::new("cmake").arg("--build").arg(&build));
    std::fs::read(build.join("hello.elf")).unwrap()
}

#[task::test]
async fn runs_sdk_c_program_through_the_posix_layer() {
    let time = Box::leak(Box::new(
        time::Manager::new(&testing_support::TimeDevice).unwrap(),
    ));
    let tasks = task::test_manager();
    let users = Box::leak(Box::new(users::Manager::new()));
    let parent = tasks.get_current_task_identifier().await;
    let device = file_system::MemoryDevice::<512>::new_static(256 * 1024);
    let fs = little_fs::FileSystem::new_format(device, little_fs::Options::new(256)).unwrap();
    let vfs = Box::leak(Box::new(
        virtual_file_system::initialize(tasks, users, time, fs).unwrap(),
    ));
    let mut context = ExecutableContext::new(tasks, users, vfs, time);
    context.formats = const { &[LOADER.registration()] };
    let context = Box::leak(Box::new(context));

    vfs.create_directory(parent, &"/devices").await.unwrap();
    vfs.create_directory(parent, &"/devices/random")
        .await
        .unwrap();
    vfs.mount_character_device(parent, "/devices/random/source", FixtureEntropy)
        .await
        .unwrap();

    let create = Flags::new(AccessFlags::READ_WRITE, Some(CreateFlags::Create), None);

    // Integer-only printf (the default) and the double variant, each a separate build.
    for format in ["integer", "double"] {
        let bytes = build_sdk_example(format);
        let program_path = format!("/program-{format}");
        let output_path = format!("/output-{format}");
        let mut program = File::open(vfs, parent, program_path.as_str(), create)
            .await
            .unwrap();
        assert_eq!(program.write(&bytes).await.unwrap(), bytes.len());
        program.close(vfs).await.unwrap();
        vfs.set_permissions(parent, program_path.as_str(), Permissions::EXECUTABLE)
            .await
            .unwrap();

        let input = File::open(vfs, parent, "/input", create).await.unwrap();
        let mut output = File::open(vfs, parent, output_path.as_str(), create)
            .await
            .unwrap();

        let standard = Standard::new(
            context,
            input,
            output.duplicate().await.unwrap(),
            output.duplicate().await.unwrap(),
        );
        // The program exits with the status given as its first argument.
        let handle = execute(
            context,
            program_path.as_str(),
            vec!["3".into()],
            standard,
            None,
        )
        .await
        .unwrap();
        assert_eq!(handle.join().await, 3);
        assert_eq!(MAPPINGS.load(Ordering::Relaxed), 0);

        output.set_position(&Position::Start(0)).await.unwrap();
        let mut text = Vec::new();
        let mut chunk = [0; 256];
        loop {
            let count = output.read(&mut chunk).await.unwrap();
            if count == 0 {
                break;
            }
            text.extend_from_slice(&chunk[..count]);
        }
        let text = String::from_utf8(text).unwrap();
        println!("{text}");
        assert!(text.contains("hello from Xila, 1 argument(s)\n"), "{text}");
        assert!(text.contains("entropy: a5a5a5a5\n"), "{text}");
        assert!(text.ends_with("hello: ok\n"), "{text}");

        // The program removed everything it created, except the stream it never closed:
        // exit flushed that one.
        assert!(vfs.get_statistics(&"/sdk-hello").await.is_err());
        let mut flushed = File::open(vfs, parent, "/sdk-exit.txt", AccessFlags::Read.into())
            .await
            .unwrap();
        let mut buffer = [0; 32];
        let count = flushed.read(&mut buffer).await.unwrap();
        assert_eq!(&buffer[..count], b"flushed at exit\n");
        flushed.close(vfs).await.unwrap();
        output.close(vfs).await.unwrap();
    }
}
