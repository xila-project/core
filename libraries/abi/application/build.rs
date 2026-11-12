use std::{env, path::PathBuf};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut config = cbindgen::Config {
        language: cbindgen::Language::C,
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
        ("DirectoryIterator", "XilaDirectoryIterator"),
    ] {
        config.export.rename.insert(rust.into(), c.into());
    }
    config.enumeration.rename_variants = cbindgen::RenameRule::ScreamingSnakeCase;
    config.enumeration.prefix_with_name = true;
    cbindgen::Builder::new()
        .with_crate(&root)
        .with_config(config)
        .generate()
        .unwrap()
        .write_to_file(out.join("application.h"));
    println!("cargo:include={}", out.display());
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
