//! Detects the RISC-V floating-point ABI, which `rustc` does not expose to `cfg` on stable.
//!
//! `cfg!(target_feature = "f")` and `"d"` are false for `riscv32imafc` because the
//! floating-point features are unstable, so the loader would reject correct hard-float
//! images and accept soft-float ones. The ABI is recovered from the target name instead.
//! Unlike `stackful`, which cares which float registers exist, the loader needs the
//! calling convention, which `-C target-feature` flags do not change.
use std::env;

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
}
