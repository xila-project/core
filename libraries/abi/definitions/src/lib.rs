#![no_std]

extern crate alloc;

use synchronization::once_lock::OnceLock;

pub struct RuntimeContext {
    pub virtual_file_system: &'static virtual_file_system::VirtualFileSystem,
    pub time_manager: &'static ::time::Manager<'static>,
}

static RUNTIME_CONTEXT: OnceLock<RuntimeContext> = OnceLock::new();

/// Install the process-wide context used only by C ABI entry points.
pub fn initialize(runtime_context: RuntimeContext) {
    RUNTIME_CONTEXT.get_or_init(|| runtime_context);
}

pub(crate) fn runtime_context() -> &'static RuntimeContext {
    RUNTIME_CONTEXT
        .try_get()
        .expect("ABI runtime context not initialized")
}

mod file_system;
mod memory;
mod string;
mod task;
mod time;
mod user;

pub use file_system::*;
pub use memory::*;
pub use string::*;
pub use task::*;
pub use time::*;
pub use user::*;

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    drivers_std::memory::instantiate_global_allocator!();
}
