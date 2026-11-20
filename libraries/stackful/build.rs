use std::env;

/// Emits `stackful_fp_control` when the target has floating-point control state
/// that must stay local to each coroutine (see `src/fp.rs`).
fn main() {
    println!("cargo:rustc-check-cfg=cfg(stackful_fp_control)");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    // Rust does not expose the unstable floating-point target features of Arm
    // and RISC-V in cfg. Use the built-in target defaults and explicit flags.
    let mut float = target.contains("eabihf")
        || target.starts_with("riscv32imaf")
        || features
            .split(',')
            .any(|f| f == "f" || f == "fp" || f.starts_with("vfp"));
    for flag in flags.split(['\x1f', ',', '=']) {
        if flag == "+f"
            || flag.starts_with("+vfp")
            || flag.starts_with("+fp-arm")
            || flag == "+fpregs"
        {
            float = true;
        }
        if flag == "-f" {
            float = false;
        }
    }
    if matches!(arch.as_str(), "x86_64" | "aarch64")
        || (float && matches!(arch.as_str(), "arm" | "riscv32" | "xtensa"))
    {
        println!("cargo:rustc-cfg=stackful_fp_control");
    }
}
