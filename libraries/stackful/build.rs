use std::env;
fn main() {
    println!("cargo:rustc-check-cfg=cfg(stackful_corosensei)");
    println!("cargo:rustc-check-cfg=cfg(stackful_corosensei_probe)");
    println!("cargo:rustc-check-cfg=cfg(stackful_float)");
    println!("cargo:rustc-check-cfg=cfg(stackful_fpv5)");
    println!("cargo:rustc-check-cfg=cfg(stackful_xtensa_fp)");
    println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
    let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let features = env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    // Rust does not expose unstable floating-point target features in cfg on
    // these architectures. Include built-in target defaults and explicit flags.
    let flags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let mut float = env::var_os("CARGO_FEATURE_FPU").is_some()
        || target.contains("eabihf")
        || target.starts_with("riscv32imaf")
        || features
            .split(',')
            .any(|f| f == "f" || f.starts_with("vfp"));
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
    // The RISC-V float frame saves single-precision registers. With the D
    // extension they are 64 bits wide and `fsw` would drop their upper halves.
    let double = features.split(',').any(|f| f == "d")
        || flags.split(['\x1f', ',', '=']).any(|f| f == "+d")
        || target.starts_with("riscv32imafd")
        || target.starts_with("riscv32gc");
    if arch == "riscv32" && double {
        panic!(
            "stackful: RISC-V double-precision (D extension) float registers are not supported yet"
        );
    }
    if arch == "arm"
        && !(target.starts_with("thumbv6m-")
            || target.starts_with("thumbv7em-")
            || target.starts_with("thumbv8m.main-"))
    {
        panic!("stackful: only Cortex-M thumbv6m, thumbv7em and thumbv8m.main are supported");
    }
    let supported = match arch.as_str() {
        "x86_64" => target.ends_with("-linux-gnu"),
        "aarch64" => true,
        "riscv32" => !float || target == "riscv32imafc-unknown-none-elf",
        // Rust's soft-float ABI does not guarantee preservation of the softfp
        // C caller's callee-saved VFP registers. Retain the explicit saves.
        "arm" => !float || target.ends_with("eabihf"),
        // Windowed ABI, ESP32, ESP32-S2 and ESP32-S3 (fork branch only).
        "xtensa" => true,
        _ => false,
    };
    if supported
        && env::var_os("CARGO_FEATURE_COROSENSEI_PROBE").is_some()
        && env::var_os("CARGO_CFG_STACKFUL_COROSENSEI_PROBE").is_some()
    {
        println!("cargo:rustc-cfg=stackful_corosensei");
    }
    // ESP32 and ESP32-S3 have the single-precision FPU; the ESP32-S2 has none.
    if arch == "xtensa" && features.split(',').any(|f| f == "fp") {
        println!("cargo:rustc-cfg=stackful_xtensa_fp");
    }
    if float {
        println!("cargo:rustc-cfg=stackful_float");
        // Cortex-M33 class cores have an FPv5 unit; Cortex-M4F has FPv4.
        if target.starts_with("thumbv8m.main-") {
            println!("cargo:rustc-cfg=stackful_fpv5");
        }
    }
}
