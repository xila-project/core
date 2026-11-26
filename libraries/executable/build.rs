//! Builds the linked C fixtures of `tests/linked.rs`: two copies of one C program, each
//! localized with `llvm-objcopy` (every symbol except the entry made local, as the SDK's
//! `link-archive` command does) and archived into one static library that the test binary
//! links. Only on Linux x86-64 with the `native-linked` feature, and only when clang,
//! ld.lld, llvm-objcopy and llvm-ar are available; everywhere else it writes a fallback
//! module and builds nothing, so the test skips the fixtures at runtime.
//!
//! With the `native-dev` feature as well (the dual-mode suite of `tests/sdk.rs`) and a
//! built host SDK (`XILA_SDK_DIRECTORY` or `target/sdk/x86_64-unknown-linux-gnu`), it
//! also compiles the SDK's hexdump example against the SDK's packaged headers and
//! localizes it with `tools/sdk/build.py link-archive` into `libxila_linked_hexdump.a`,
//! which the test binary links and mounts as `/linked/hexdump`. Without a built SDK it
//! writes a fallback module and emits no `xila_linked_hexdump` cfg, so the test skips
//! the linked mode instead of failing the build (unless `XILA_REQUIRE_LINKED_HEXDUMP=1`
//! is set, which fails the build loudly if the linked archive cannot be built).

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The two fixture programs: a compile-time label and the entry symbol each localized
/// object exposes.
const FIXTURES: [(&str, &str); 2] = [
    ("alpha", "xila_c_alpha_entry"),
    ("beta", "xila_c_beta_entry"),
];

/// The SDK's hexdump example and the entry symbol its localized archive exposes.
const HEXDUMP_SOURCE: &str = "../abi/application/sdk/example/hexdump/hexdump.c";
const HEXDUMP_ENTRY: &str = "xila_linked_hexdump";
const HOST_TARGET: &str = "x86_64-unknown-linux-gnu";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=tests/linked_c");
    println!("cargo:rerun-if-changed={HEXDUMP_SOURCE}");
    println!("cargo:rerun-if-changed=../../tools/sdk/build.py");
    // Watch the default host SDK directory unconditionally so building the SDK after a
    // fallback build retriggers build.rs instead of keeping the cached fallback.
    println!("cargo:rerun-if-changed=../../target/sdk/{HOST_TARGET}");
    // The tool lookup goes through PATH (and clang's -print-prog-name).
    println!("cargo:rerun-if-env-changed=PATH");
    println!("cargo:rerun-if-env-changed=XILA_SDK_DIRECTORY");
    println!("cargo:rerun-if-env-changed=XILA_REQUIRE_LINKED_HEXDUMP");
    // Declared unconditionally, so rustc and clippy never warn about the cfg below.
    println!("cargo:rustc-check-cfg=cfg(xila_linked_hexdump)");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let linked = env::var_os("CARGO_FEATURE_NATIVE_LINKED").is_some();
    let native_dev = env::var_os("CARGO_FEATURE_NATIVE_DEV").is_some();
    let require_linked_hexdump = env::var("XILA_REQUIRE_LINKED_HEXDUMP").as_deref() == Ok("1");
    let host = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
        && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64");
    if !linked || !host {
        write_fallback(&out_dir);
        write_hexdump_module(&out_dir, false);
        return;
    }
    let Some(tools) = find_tools() else {
        if native_dev && require_linked_hexdump {
            panic!(
                "XILA_REQUIRE_LINKED_HEXDUMP=1: missing one of clang, ld.lld, llvm-objcopy, llvm-ar"
            );
        }
        write_fallback(&out_dir);
        write_hexdump_module(&out_dir, false);
        return;
    };

    // The generated application.h matches the host's `XilaStartup` layout exactly; the
    // checked-in SDK header is the fallback.
    let include = env::var_os("DEP_XILA_ABI_APPLICATION_INCLUDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../abi/application/include/xila"));
    let source = manifest.join("tests/linked_c/program.c");

    let mut members = Vec::new();
    for (label, entry) in FIXTURES {
        let object = out_dir.join(format!("linked_c_{label}.o"));
        let combined = out_dir.join(format!("linked_c_{label}.combined.o"));
        let renamed = out_dir.join(format!("linked_c_{label}.renamed.o"));
        let localized = out_dir.join(format!("linked_c_{label}.localized.o"));
        run(Command::new(&tools.clang)
            .arg("-c")
            .arg(&source)
            .arg("-o")
            .arg(&object)
            .arg(format!("-DLABEL=\"{label}\""))
            .arg("-std=gnu11")
            .arg("-Os")
            // -fPIE like every SDK object: the fixtures link into this PIE test binary.
            .arg("-fPIE")
            .arg("-ffunction-sections")
            .arg("-fdata-sections")
            .arg("-fno-stack-protector")
            .arg("-fno-unwind-tables")
            .arg("-fno-asynchronous-unwind-tables")
            .arg(format!("-I{}", include.display())));
        run(Command::new(&tools.lld)
            .arg("-r")
            .arg("-o")
            .arg(&combined)
            .arg("--undefined=xila_linked_start")
            .arg(&object));
        run(Command::new(&tools.objcopy)
            .arg(format!("--redefine-sym=xila_linked_start={entry}"))
            .arg(&combined)
            .arg(&renamed));
        run(Command::new(&tools.objcopy)
            .arg(format!("--keep-global-symbol={entry}"))
            .arg(&renamed)
            .arg(&localized));
        members.push(localized);
    }
    let archive = out_dir.join("libxila_linked_c_fixtures.a");
    run(Command::new(&tools.ar)
        .arg("rcs")
        .arg(&archive)
        .args(&members));

    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=xila_linked_c_fixtures");
    fs::write(
        out_dir.join("linked_c_fixtures.rs"),
        format!(
            "// Generated by build.rs: the localized C fixtures of tests/linked.rs.\n\
             pub const AVAILABLE: bool = true;\n\
             unsafe extern \"C\" {{\n\
             {entries}\
             }}\n",
            entries = FIXTURES
                .iter()
                .map(|(_, entry)| format!(
                    "    pub fn {entry}(startup: *const abi_application::XilaStartup) -> i32;\n"
                ))
                .collect::<String>(),
        ),
    )
    .unwrap();

    // The hexdump example archive, for the dual-mode suite of tests/sdk.rs: only with
    // native-dev too (that test is the only consumer) and a built host SDK. Otherwise
    // the test skips the linked mode instead of failing the build, unless
    // XILA_REQUIRE_LINKED_HEXDUMP=1 is set (as in test:native-loader).
    if native_dev {
        match build_hexdump_archive(&out_dir, &manifest, &tools) {
            Ok(()) => {
                println!("cargo:rustc-link-search=native={}", out_dir.display());
                println!("cargo:rustc-link-lib=static=xila_linked_hexdump");
                println!("cargo:rustc-cfg=xila_linked_hexdump");
                write_hexdump_module(&out_dir, true);
            }
            Err(reason) => {
                if require_linked_hexdump {
                    panic!(
                        "XILA_REQUIRE_LINKED_HEXDUMP=1: failed to build libxila_linked_hexdump.a: {reason}"
                    );
                }
                write_hexdump_module(&out_dir, false);
            }
        }
    } else {
        write_hexdump_module(&out_dir, false);
    }
}

/// Compile the SDK's hexdump example with the packaged SDK's headers and localize it
/// with `tools/sdk/build.py link-archive`, exactly as a firmware build would. Returns
/// `Err(reason)`, building nothing, when no host SDK is available or a tool is missing.
fn build_hexdump_archive(out_dir: &Path, manifest: &Path, tools: &Tools) -> Result<(), String> {
    let Some(sdk) = host_sdk_directory(manifest) else {
        return Err(format!(
            "no complete {HOST_TARGET} SDK found in XILA_SDK_DIRECTORY or target/sdk/{HOST_TARGET}"
        ));
    };
    // link-archive needs all of these; check them here so a partial toolset skips the
    // archive instead of failing the build.
    for name in [
        "ld.lld",
        "llvm-objcopy",
        "llvm-ar",
        "llvm-nm",
        "llvm-readobj",
    ] {
        if find_tool(name).is_none() {
            return Err(format!("missing tool {name}"));
        }
    }
    let Some(python) = which("python3") else {
        return Err("missing python3 on PATH".into());
    };
    let source = manifest.join(HEXDUMP_SOURCE);
    let build_py = manifest.join("../../tools/sdk/build.py");
    for file in [
        "sdk.json",
        "cmake/xila-toolchain.cmake",
        "lib/crt0_linked.o",
        "lib/libxila_posix.a",
        "lib/libxila.a",
        "lib/libc.a",
    ] {
        println!("cargo:rerun-if-changed={}", sdk.join(file).display());
    }

    let resource = Command::new(&tools.clang)
        .arg("--target=x86_64-unknown-none-elf")
        .arg("-print-resource-dir")
        .clean_env()
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
    let Some(resource) = resource else {
        return Err("clang -print-resource-dir failed".into());
    };
    let object = out_dir.join("hexdump.o");
    // The same flags and include layout as the SDK's toolchain file, over the packaged
    // headers, so the object matches the one CMake builds for hexdump.elf.
    run(Command::new(&tools.clang)
        .arg("--target=x86_64-unknown-none-elf")
        .arg("-c")
        .arg(&source)
        .arg("-o")
        .arg(&object)
        .arg("-std=gnu11")
        .arg("-Os")
        // -fPIE like every SDK object: the archive links into this PIE test binary.
        .arg("-fPIE")
        .arg("-ffunction-sections")
        .arg("-fdata-sections")
        .arg("-fno-stack-protector")
        .arg("-fno-unwind-tables")
        .arg("-fno-asynchronous-unwind-tables")
        .arg("-D_GNU_SOURCE")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Werror")
        .arg("-nostdinc")
        .arg(format!("-isystem{}/include/posix", sdk.display()))
        .arg(format!("-isystem{}/include", sdk.display()))
        .arg(format!("-isystem{}/include", resource)));
    let archive = out_dir.join("libxila_linked_hexdump.a");
    run(Command::new(python)
        .arg(&build_py)
        .arg("link-archive")
        .arg("--sdk")
        .arg(&sdk)
        .arg("--entry")
        .arg(HEXDUMP_ENTRY)
        .arg("--output")
        .arg(&archive)
        .arg(&object));
    Ok(())
}

/// The built host SDK: `XILA_SDK_DIRECTORY` when set (must be a complete host SDK),
/// else the newest `xila-sdk-*` directory under the workspace's `target/sdk` for the
/// host target.
fn host_sdk_directory(manifest: &Path) -> Option<PathBuf> {
    if let Some(directory) = env::var_os("XILA_SDK_DIRECTORY") {
        let directory = PathBuf::from(directory);
        return sdk_is_complete(&directory).then_some(directory);
    }
    let parent = manifest.join("../../target/sdk").join(HOST_TARGET);
    let mut found: Vec<PathBuf> = fs::read_dir(parent)
        .ok()?
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("xila-sdk-") && name.ends_with(HOST_TARGET))
        })
        .collect();
    found.sort();
    found.pop().filter(|path| sdk_is_complete(path))
}

/// The files link-archive links a program against, for the host target.
fn sdk_is_complete(sdk: &Path) -> bool {
    let target_matches = fs::read_to_string(sdk.join("sdk.json"))
        .ok()
        .is_some_and(|text| {
            let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            compact.contains(&format!("\"target\":\"{HOST_TARGET}\""))
        });
    target_matches
        && [
            "lib/crt0_linked.o",
            "lib/libxila_posix.a",
            "lib/libxila.a",
            "lib/libc.a",
        ]
        .iter()
        .all(|file| sdk.join(file).is_file())
}

/// The generated module of tests/sdk.rs: the linked hexdump entry when the archive was
/// built, a stand-in plus `AVAILABLE = false` otherwise.
fn write_hexdump_module(out_dir: &Path, available: bool) {
    let text = if available {
        format!(
            "// Generated by build.rs: the localized SDK hexdump example of tests/sdk.rs.\n\
             unsafe extern \"C\" {{\n\
             pub fn {HEXDUMP_ENTRY}(startup: *const abi_application::XilaStartup) -> i32;\n\
             }}\n"
        )
    } else {
        let entry = HEXDUMP_ENTRY;
        format!(
            "// Generated by build.rs: no linked hexdump (no host SDK or no tools).\n\
             /// Never called: the xila_linked_hexdump cfg is not set, so tests/sdk.rs\n\
             /// does not compile this module.\n\
             ///\n\
             /// # Safety\n\
             /// A stand-in for the localized C entry.\n\
             #[allow(dead_code)]\n\
             pub unsafe extern \"C\" fn {entry}(_startup: *const abi_application::XilaStartup) -> i32 {{\n\
             unreachable!(\"the linked hexdump archive was not built\")\n\
             }}\n"
        )
    };
    fs::write(out_dir.join("hexdump_linked.rs"), text).unwrap();
}

/// The fallback module: the test compiles and links without the C fixtures and skips
/// them at runtime.
fn write_fallback(out_dir: &Path) {
    let mut text = String::from(
        "// Generated by build.rs: no C fixtures (tools or target missing).\n\
                      pub const AVAILABLE: bool = false;\n",
    );
    for (_, entry) in FIXTURES {
        text.push_str("/// Never called: the test skips the fixtures when unavailable.\n");
        text.push_str("///\n/// # Safety\n/// A stand-in for the localized C entry.\n");
        text.push_str(&format!(
            "pub unsafe extern \"C\" fn {entry}(_startup: *const abi_application::XilaStartup) -> i32 {{\n    unreachable!(\"C fixtures were not built\")\n}}\n"
        ));
    }
    fs::write(out_dir.join("linked_c_fixtures.rs"), text).unwrap();
}

struct Tools {
    clang: PathBuf,
    lld: PathBuf,
    objcopy: PathBuf,
    ar: PathBuf,
}

/// The four tools, on PATH or next to clang; `None` when one is missing.
fn find_tools() -> Option<Tools> {
    let clang = find_tool("clang")?;
    Some(Tools {
        lld: find_tool("ld.lld")?,
        objcopy: find_tool("llvm-objcopy")?,
        ar: find_tool("llvm-ar")?,
        clang,
    })
}

fn find_tool(name: &str) -> Option<PathBuf> {
    if let Some(found) = which(name) {
        return Some(found);
    }
    // Distributions install versioned LLVM directories; ask clang where its tools are.
    let output = Command::new("clang")
        .arg(format!("-print-prog-name={name}"))
        .clean_env()
        .output()
        .ok()?;
    let candidate = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(&candidate);
    if output.status.success() && path.is_absolute() && path.exists() {
        Some(path)
    } else {
        None
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Run a command, without a stale LD_LIBRARY_PATH that can break clang's driver.
trait CleanEnv {
    fn clean_env(&mut self) -> &mut Self;
}

impl CleanEnv for Command {
    fn clean_env(&mut self) -> &mut Self {
        if let Some(paths) = env::var_os("PATH") {
            self.env("PATH", paths);
        }
        self.env_remove("LD_LIBRARY_PATH");
        self
    }
}

fn run(command: &mut Command) {
    let status = command.clean_env().status().unwrap();
    assert!(status.success(), "{command:?} failed with {status}");
}
