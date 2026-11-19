//! Internal libc-style helpers for the C libraries linked into Xila.
//!
//! LVGL and LittleFS expect a C library (`malloc`, `memcpy`, `strlen`, ...).
//! Their Rust glue (`graphics`' LVGL hooks and `little_fs`' stubs) forwards to
//! the functions of this crate, which allocate through the system
//! `memory` manager and implement the memory and string primitives.
//!
//! This crate is plain Rust: it exports no C symbols, ships no header and is
//! not an application ABI. The public native ABI is `xila-abi-application`.

#![no_std]

extern crate alloc;

mod memory;
mod string;

pub use memory::*;
pub use string::*;

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    testing_support::instantiate_global_allocator!();
}
