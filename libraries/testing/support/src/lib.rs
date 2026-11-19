//! Host test helpers for Core crates that the full test environment depends on.
//!
//! Keep the environment's runtime dependencies out of this crate so their
//! unit-test binaries do not link a second copy of the crate under test.

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
