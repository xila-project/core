#![no_std]

extern crate alloc;

mod cursor;
mod directory;
mod error;
mod file;
mod file_system;
mod hierarchy;
mod home;
mod item;
mod r#macro;
mod operation;
mod pipe;
mod readiness;
mod synchronous_directory;
mod synchronous_file;

pub use directory::*;
pub use error::*;
use exported_file_system::Flags;
pub use file::*;
pub use file_system::*;
pub use hierarchy::*;
pub use item::*;
pub use pipe::PIPE_BUF;
pub use readiness::*;
pub use synchronous_directory::*;
pub use synchronous_file::*;

pub extern crate file_system as exported_file_system;

/// Run synchronous metadata once. Busy is an error, not an implicit wait.
pub async fn poll<O>(operation: impl FnOnce() -> Result<O>) -> Result<O> {
    operation()
}

pub fn blocking_operation<O>(_flags: Flags, operation: impl FnOnce() -> Result<O>) -> Result<O> {
    operation()
}

#[cfg(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32",
            target_arch = "xtensa"
        ),
        target_os = "none"
    )
))]
use stackful::{suspend_on, suspend_on_cancellable};

#[cfg(not(any(
    all(
        any(target_arch = "x86_64", target_arch = "aarch64"),
        target_os = "linux"
    ),
    all(
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32",
            target_arch = "xtensa"
        ),
        target_os = "none"
    )
)))]
use task::{block_on as suspend_on, block_on as suspend_on_cancellable};
