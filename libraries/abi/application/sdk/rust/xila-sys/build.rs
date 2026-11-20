use std::{env, fs, path::PathBuf, process::Command};

const SCRIPT: &str = "xila-static-pie.ld";

fn main() {
    println!("cargo::rustc-check-cfg=cfg(xila_static_pie)");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed={SCRIPT}");

    if env::var_os("CARGO_FEATURE_LINK").is_none() {
        return;
    }
    let target = env::var("TARGET").unwrap();
    let alignment = match target.as_str() {
        "x86_64-unknown-linux-gnu" => 4096, // The host separates permissions with mprotect.
        "thumbv7em-none-eabihf" | "riscv32imac-unknown-none-elf" => 16,
        _ => {
            println!(
                "cargo::warning=xila-sys: target {target} has no Xila static PIE link configuration"
            );
            return;
        }
    };

    // Raw link arguments in rlib metadata require nightly, as does rebuilding
    // the guest's PIC sysroot. Stable builds can still check all features.
    let version = Command::new(env::var_os("RUSTC").unwrap())
        .arg("--version")
        .output()
        .expect("query rustc version");
    assert!(version.status.success(), "could not query rustc version");
    if !String::from_utf8(version.stdout)
        .unwrap()
        .contains("-nightly")
    {
        println!(
            "cargo::warning=xila-sys: static PIE link arguments require the nightly compiler pinned in Core's mise.toml"
        );
        return;
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let script = out.join(SCRIPT);
    fs::write(&script, include_str!("xila-static-pie.ld")).unwrap();
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
        &format!("-T{}", script.display()),
    ] {
        use std::fmt::Write;
        writeln!(attributes, "#[link(name = {arg:?}, kind = \"link-arg\")]").unwrap();
    }
    attributes.push_str("unsafe extern \"C\" {}\n");
    fs::write(out.join("link.rs"), attributes).unwrap();
}
