//! Stackful, executor-affine native coroutines. See README.md for the safety contract.
#![no_std]
// The floating-point control registers are read with Xtensa inline assembly.
#![cfg_attr(
    all(target_arch = "xtensa", stackful_fp_control),
    feature(asm_experimental_arch)
)]

extern crate alloc;
#[cfg(target_os = "linux")]
extern crate std;

#[cfg(target_arch = "wasm32")]
compile_error!("stackful: Wasm cannot switch native stacks");
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv32",
    target_arch = "xtensa",
    target_arch = "wasm32"
)))]
compile_error!("stackful: unsupported architecture");
#[cfg(all(not(target_os = "linux"), not(target_os = "none")))]
compile_error!("stackful: supported operating systems are Linux and bare metal");

#[cfg(all(
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv32",
        target_arch = "xtensa"
    ),
    any(target_os = "linux", target_os = "none")
))]
mod runtime;
#[cfg(all(
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv32",
        target_arch = "xtensa"
    ),
    any(target_os = "linux", target_os = "none")
))]
pub use runtime::*;
