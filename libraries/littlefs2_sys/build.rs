use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let littlefs_path = if cfg!(feature = "unstable-littlefs-patched") {
        "littlefs-patched"
    } else {
        "littlefs"
    };
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Patch lfs.h to remove the lfs_util import because clang fails to locate the
    // libraries for the custom target (especially string.h)
    // Compilation before that succeeds because it's using gcc,
    // which comes as a distribution with these utils.
    // Turns out lfs_utils is not used in lfs.h, and clang properly finds stdint.h and stdbool,
    // but not string.h
    let lfs_h =
        std::fs::read_to_string(format!("{littlefs_path}/lfs.h")).expect("Reading lfs.h succeeds");
    println!("cargo::rerun-if-changed={littlefs_path}/lfs.h");
    let out_lfs_h = out_path.join("lfs.h");
    std::fs::write(
        &out_lfs_h,
        lfs_h.replace(
            r##"#include "lfs_util.h""##,
            "#include <stdint.h>\n#include <stdbool.h>",
        ),
    )
    .expect("Failed to write lfs.h");

    let mut builder = cc::Build::new();
    if env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "xtensa") {
        builder.flag("-mlongcalls");
    }
    let builder = builder
        .flag("-std=c99")
        .flag("-DLFS_NO_DEBUG")
        .flag("-DLFS_NO_WARN")
        .flag("-DLFS_NO_ERROR")
        .include(&out_path)
        .include(littlefs_path)
        .file(format!("{littlefs_path}/lfs.c"))
        .file(format!("{littlefs_path}/lfs_util.c"));

    // Bare-metal targets have no C library: use the compiler's own headers.
    let freestanding = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "none");
    if freestanding {
        builder.flag("-ffreestanding");
    }

    let target = env::var("TARGET").unwrap_or_default();
    let target_abi = env::var("CARGO_CFG_TARGET_ABI").unwrap_or_default();
    let riscv_flags = riscv_compiler_flags(&target, &target_abi);
    if let Some((architecture_flag, abi_flag)) = &riscv_flags {
        builder.flag(architecture_flag).flag(abi_flag);
    }

    #[cfg(feature = "software-intrinsics")]
    let builder = builder.flag("-DLFS_NO_INTRINSICS");

    #[cfg(not(feature = "assertions"))]
    let builder = builder.flag("-DLFS_NO_ASSERT");

    #[cfg(feature = "trace")]
    let builder = builder.flag("-DLFS_YES_TRACE");

    #[cfg(not(feature = "malloc"))]
    builder.flag("-DLFS_NO_MALLOC");

    // Keep Xila's configuration with Core even when Core is consumed over Git.
    // An explicit override remains available for downstream targets.
    let lfs_config = env::var("LITTLEFS_CONFIG").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../little_fs/include/little_fs_config.h")
            .to_string_lossy()
            .into_owned()
    });
    builder.define("LFS_CONFIG", Some(lfs_config.as_str()));
    println!("cargo::rerun-if-env-changed=LITTLEFS_CONFIG");
    println!("cargo::rerun-if-changed={}", lfs_config);

    #[cfg(feature = "multiversion")]
    let builder = builder.flag("-DLFS_MULTIVERSION");

    builder.compile("lfs-sys");

    let bindgen = bindgen::Builder::default()
        .header(out_lfs_h.into_os_string().into_string().unwrap())
        .clang_arg("-std=c99")
        .clang_arg("-DLFS_NO_DEBUG")
        .clang_arg("-DLFS_NO_WARN")
        .clang_arg("-DLFS_NO_ERROR");

    #[cfg(feature = "multiversion")]
    let bindgen = bindgen.clang_arg("-DLFS_MULTIVERSION");

    let mut bindings_builder = bindgen
        .derive_default(true)
        .use_core()
        .allowlist_item("lfs_.*")
        .allowlist_item("LFS_.*");

    if freestanding {
        bindings_builder = bindings_builder.clang_arg("-ffreestanding");
    }

    if let Some((architecture_flag, abi_flag)) = &riscv_flags {
        bindings_builder = bindings_builder
            .clang_arg(architecture_flag)
            .clang_arg(abi_flag);
    }

    if target == "wasm32-unknown-unknown" {
        bindings_builder = bindings_builder.clang_arg("-fvisibility=default");
    }

    let bindings = bindings_builder
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    Ok(())
}

/// Derives `-march=` and `-mabi=` flags for RISC-V targets from the Rust target
/// triple and `CARGO_CFG_TARGET_ABI`.
///
/// Both `cc` and `bindgen` translate bare-metal Rust triples such as
/// `riscv32imafc-unknown-none-elf` to `riscv32-unknown-none-elf`, and `cc`
/// defaults bare-metal GNU RISC-V builds to `-mabi=ilp32`. Passing explicit
/// architecture and ABI flags ensures C objects match Rust's floating-point
/// calling convention (for example `ilp32f` on `riscv32imafc-unknown-none-elf`).
fn riscv_compiler_flags(target: &str, target_abi: &str) -> Option<(String, String)> {
    let architecture = target.split('-').next()?;
    let (width, extensions) = if let Some(rest) = architecture.strip_prefix("riscv32") {
        (32u8, rest)
    } else {
        let rest = architecture.strip_prefix("riscv64")?;
        (64u8, rest)
    };

    if extensions.is_empty()
        || !matches!(extensions.as_bytes()[0], b'i' | b'e' | b'g')
        || !extensions
            .bytes()
            .all(|byte| matches!(byte, b'i' | b'e' | b'm' | b'a' | b'f' | b'd' | b'c' | b'g'))
    {
        return None;
    }

    let abi = match target_abi {
        "ilp32" | "ilp32e" | "ilp32f" | "ilp32d" | "lp64" | "lp64f" | "lp64d" => target_abi,
        _ if width == 32 => {
            if extensions.starts_with('e') {
                "ilp32e"
            } else if extensions.contains('g') || extensions.contains('d') {
                "ilp32d"
            } else if extensions.contains('f') {
                "ilp32f"
            } else {
                "ilp32"
            }
        }
        _ => {
            if extensions.contains('g') || extensions.contains('d') {
                "lp64d"
            } else if extensions.contains('f') {
                "lp64f"
            } else {
                "lp64"
            }
        }
    };

    Some((format!("-march=rv{width}{extensions}"), format!("-mabi={abi}")))
}
