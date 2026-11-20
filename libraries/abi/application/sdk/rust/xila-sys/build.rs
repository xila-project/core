use std::{env, fs, path::PathBuf, process::Command};

const SCRIPT: &str = "xila-static-pie.ld";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(xila_static_pie)");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={SCRIPT}");

    if env::var_os("CARGO_FEATURE_RUNTIME").is_none() {
        return;
    }
    let alignment = match env::var("TARGET").unwrap().as_str() {
        "x86_64-unknown-linux-gnu" => 4096, // The host separates permissions with mprotect.
        "thumbv7em-none-eabihf" | "riscv32imac-unknown-none-elf" => 16,
        _ => return,
    };

    // Raw link arguments in rlib metadata require nightly. Stable builds can
    // still use and check the runtime API. Native guest builds use nightly
    // to rebuild the PIC sysroot.
    let version = Command::new(env::var_os("RUSTC").unwrap())
        .arg("--version")
        .output()
        .expect("query rustc version");
    assert!(version.status.success(), "could not query rustc version");
    if !String::from_utf8(version.stdout)
        .unwrap()
        .contains("-nightly")
    {
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join(SCRIPT), include_str!("xila-static-pie.ld")).unwrap();
    println!("cargo::rustc-link-search=native={}", out.display());
    println!("cargo::rustc-cfg=xila_static_pie");

    // Cargo's rustc-link-arg instruction only affects this package's targets.
    // These attributes put the arguments in the rlib so rustc forwards them
    // when linking the downstream guest.
    let mut attributes = String::new();
    for arg in [
        "-pie",
        "--no-dynamic-linker",
        "-Bsymbolic",
        "-z",
        "max-page-size=16",
        "--gc-sections",
        "--build-id=none",
        &format!("--defsym=XILA_DATA_ALIGN={alignment}"),
        &format!("-T{SCRIPT}"),
    ] {
        use std::fmt::Write;
        writeln!(attributes, "#[link(name = {arg:?}, kind = \"link-arg\")]").unwrap();
    }
    attributes.push_str("unsafe extern \"C\" {}\n");
    fs::write(out.join("link.rs"), attributes).unwrap();
}
