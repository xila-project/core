//! Detects the RISC-V floating-point ABI, which `rustc` does not expose to `cfg` on stable.
//!
//! `cfg!(target_feature = "f")` and `"d"` are false for `riscv32imafc` because the
//! floating-point features are unstable, so the loader would reject correct hard-float
//! images and accept soft-float ones. The ABI is recovered from the target name instead.
//! Unlike `stackful`, which cares which float registers exist, the loader needs the
//! calling convention, which `-C target-feature` flags do not change.
use std::{collections::BTreeSet, env};

/// The floating-point ABI selected by `target`: `soft`, `single` or `double`.
fn target_float_abi(target: &str) -> &'static str {
    if target.starts_with("riscv32imafd")
        || target.starts_with("riscv32gc")
        || target.starts_with("riscv64gc")
    {
        "double"
    } else if target.starts_with("riscv32imaf") {
        "single"
    } else {
        "soft"
    }
}

fn main() {
    println!(
        "cargo:rustc-check-cfg=cfg(native_loader_float_abi, values(\"soft\", \"single\", \"double\"))"
    );
    println!("cargo:rerun-if-changed=build.rs");
    // The target name, for the const assertions that pin the detection per target.
    let target = env::var("TARGET").unwrap();
    println!("cargo:rustc-env=NATIVE_LOADER_TARGET={target}");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if !arch.starts_with("riscv") {
        return;
    }
    // The target alone fixes the calling convention. `-C target-feature=+f` on
    // `riscv32imac` only makes float instructions available: images stay
    // soft-float. Feature flags therefore never change the expected ABI.
    let abi = target_float_abi(&target);
    println!("cargo:rustc-cfg=native_loader_float_abi=\"{abi}\"");
    if arch == "riscv32" {
        let features = target_extensions(abi);
        println!("cargo:rustc-env=NATIVE_LOADER_RISCV_FEATURES={features}");
    }
}

// Cargo supplies stable target features after Rust applies the target defaults and flags.
// On stable Rust, F and D are absent from that list even when they are enabled.
fn target_extensions(abi: &str) -> String {
    let configured = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let mut features: BTreeSet<_> = configured
        .split(',')
        .filter(|name| !name.is_empty())
        .collect();
    let embedded = env::var("CARGO_CFG_TARGET_ABI").is_ok_and(|abi| abi == "ilp32e");
    features.insert(if embedded { "e" } else { "i" });
    if abi == "single" || abi == "double" {
        features.insert("f");
    }
    if abi == "double" {
        features.insert("d");
    }
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    for flag in flags.split('\x1f').filter_map(|flag| {
        flag.trim_start_matches("-C")
            .trim_start()
            .strip_prefix("target-feature=")
    }) {
        for feature in flag.split(',') {
            match feature {
                "+f" => {
                    features.insert("f");
                }
                "+d" => {
                    features.extend(["d", "f"]);
                }
                "-f" => {
                    features.remove("f");
                    features.remove("d");
                }
                "-d" => {
                    features.remove("d");
                }
                _ => {}
            }
        }
    }
    // Toolchains may name the component extensions as well as their parent.
    for (parent, child) in [
        ("m", "zmmul"),
        ("a", "zaamo"),
        ("a", "zalrsc"),
        ("c", "zca"),
    ] {
        if features.contains(parent) {
            features.insert(child);
        }
    }
    if features.contains("c") {
        for (float, compressed) in [("f", "zcf"), ("d", "zcd")] {
            if features.contains(float) {
                features.insert(compressed);
            }
        }
    }
    features.into_iter().collect::<Vec<_>>().join(",")
}
