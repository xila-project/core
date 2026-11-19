//! Host test helpers for Core crates that the full test environment depends on.
//!
//! This crate must not depend on `abi_definitions` or on any other crate that
//! exports `#[no_mangle]` symbols. Crates in the `environment` dependency tree
//! use it instead of `xila-testing` in their own tests: otherwise their unit-test
//! binary links a second copy of those symbols and fails. The workspace link
//! check in `ci:host-tests` (with `xila-testing/graphics`) enforces this.

#![allow(clippy::disallowed_types, reason = "host-only std test support crate")]

mod devices;
mod hash;
mod logger;
mod memory_manager;

pub use devices::{ConsoleDevice, RandomDevice, TimeDevice};
pub use hash::HashDevice;
pub use logger::Logger;
pub use memory_manager::{MemoryManager, ThreadAllocations};
pub use task::test_executor::{Executor, new_thread_executor};

#[doc(hidden)]
pub use ::memory as memory_crate;
