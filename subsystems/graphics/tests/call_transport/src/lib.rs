//! The graphics call executor's transport (`LinearMemory`) and guest memory
//! adapters (bounded C strings, widget options, grid tracks and button maps),
//! compiled without LVGL.
//!
//! `xila-graphics` links LVGL, which Miri cannot run, so its own test run
//! never checks the native pointer writes and reads these adapters make. This
//! crate includes the same source files and their tests, so that
//! `ci:abi-miri` and `ci:abi-miri-cross` run them under Miri.

#![no_std]

extern crate alloc;

/// Mirrors the `call` module of `xila-graphics`, whose paths the included
/// sources and tests use.
pub mod call;
