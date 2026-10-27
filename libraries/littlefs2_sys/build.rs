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
    let builder = builder
        .flag("-std=c99")
        .flag("-DLFS_NO_DEBUG")
        .flag("-DLFS_NO_WARN")
        .flag("-DLFS_NO_ERROR")
        .include(&out_path)
        .include(littlefs_path)
        .file(format!("{littlefs_path}/lfs.c"))
        .file(format!("{littlefs_path}/lfs_util.c"));

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

    if let Ok(target) = env::var("TARGET") {
        if target == "wasm32-unknown-unknown" {
            bindings_builder = bindings_builder.clang_arg("-fvisibility=default");
        }
    }

    let bindings = bindings_builder
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    Ok(())
}
