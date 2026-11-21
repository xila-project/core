mod generate_api;
mod generate_rust;
mod generate_status;

use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut config = cbindgen::Config {
        language: cbindgen::Language::C,
        cpp_compat: true,
        include_guard: Some("XILA_APPLICATION_H".into()),
        sys_includes: vec!["stdint.h".into(), "stddef.h".into(), "stdbool.h".into()],
        usize_is_size_t: true,
        no_includes: true,
        ..Default::default()
    };
    config
        .export
        .rename
        .insert("File".into(), "XilaFile".into());
    config
        .export
        .rename
        .insert("Directory".into(), "XilaDirectory".into());
    config
        .export
        .rename
        .insert("Status".into(), "XilaStatus".into());
    for (rust, c) in [
        ("Statistics", "XilaStatistics"),
        ("Heap", "XilaHeap"),
        ("DirectoryIterator", "XilaDirectoryIterator"),
        ("Process", "XilaProcess"),
        ("Child", "XilaChild"),
        ("Spawn", "XilaSpawn"),
        ("ThreadEntry", "XilaThreadEntry"),
        ("ThreadLibc", "XilaThreadLibc"),
        ("Mutex", "XilaMutex"),
        ("Condition", "XilaCondition"),
        ("PollEntry", "XilaPollEntry"),
    ] {
        config.export.rename.insert(rust.into(), c.into());
    }
    config
        .export
        .include
        .extend(["XilaString".into(), "XilaStartup".into(), "XilaApi".into()]);
    for name in ["Network", "NetworkAddress", "Tcp", "TcpListener", "Udp"] {
        config
            .export
            .rename
            .insert(name.into(), format!("Xila{name}"));
    }
    config
        .defines
        .insert("target_arch = arm".into(), "__arm__".into());
    config.enumeration.rename_variants = cbindgen::RenameRule::ScreamingSnakeCase;
    config.enumeration.prefix_with_name = true;
    let status = out.join("status.rs");
    generate_status::generate(&status);
    let version = out.join("version.rs");
    generate_version(&version);
    generate_api::generate(&root, &out, &config.export);
    cbindgen::Builder::new()
        .with_crate(&root)
        .with_src(status)
        .with_src(version)
        .with_config(config)
        .generate()
        .unwrap()
        .write_to_file(out.join("application.h"));
    println!("cargo:include={}", out.display());
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
}

/// Write the running Xila version as the ABI's exported constants, so the C
/// header and `xila-sys` follow Core's workspace version.
fn generate_version(output: &Path) {
    let xila_version::Version { major, minor } = xila_version::RUNNING;
    fs::write(
        output,
        format!(
            "/// Major version of the Xila release this ABI belongs to. Native images\n\
             /// record `(XILA_VERSION_MAJOR << 16) | XILA_VERSION_MINOR` in their ABI note.\n\
             pub const XILA_VERSION_MAJOR: u32 = {major};\n\
             /// Minor version of the Xila release this ABI belongs to.\n\
             pub const XILA_VERSION_MINOR: u32 = {minor};\n"
        ),
    )
    .unwrap();
}
