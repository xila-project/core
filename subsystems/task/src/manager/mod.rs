// - Dependencies
use super::*;

// - Submodules
mod lifecycle;
mod metadata;
mod properties;
mod registration;
mod relationships;
mod signals;
mod spawner;
mod utilities;

#[cfg(test)]
mod tests;

// - Re-exports

pub(crate) use metadata::*;
pub use spawner::*;

// Manager module - core Manager structure and initialization

use crate::manager::Metadata;

#[cfg(any(test, feature = "test_harness"))]
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock};

pub(crate) struct Inner {
    pub(crate) tasks: BTreeMap<TaskIdentifier, Metadata>,
    pub(crate) identifiers: BTreeMap<usize, TaskIdentifier>,
    pub(crate) spawners: BTreeMap<usize, ::embassy_executor::Spawner>,
}

unsafe impl Send for Manager {}

/// A manager for tasks.
pub struct Manager(pub(crate) RwLock<CriticalSectionRawMutex, Inner>);

#[cfg(any(test, feature = "test_harness"))]
static TEST_MANAGER: synchronization::once_lock::OnceLock<&'static Manager> =
    synchronization::once_lock::OnceLock::new();

#[cfg(any(test, feature = "test_harness"))]
static TEST_MANAGER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(any(test, feature = "test_harness"))]
pub fn test_lock() -> &'static std::sync::Mutex<()> {
    &TEST_MANAGER_LOCK
}

#[cfg(any(test, feature = "test_harness"))]
#[doc(hidden)]
pub fn test_manager() -> &'static Manager {
    TEST_MANAGER.get_or_init(|| Box::leak(Box::new(Manager::new())))
}

#[cfg(any(test, feature = "test_harness"))]
#[doc(hidden)]
pub fn reset_test_manager() -> &'static Manager {
    let manager = test_manager();
    let mut inner = embassy_futures::block_on(manager.0.write());
    inner.tasks.clear();
    inner.identifiers.clear();
    inner.spawners.clear();
    drop(inner);
    manager
}

#[cfg(any(test, feature = "test_harness"))]
pub fn initialize() -> &'static Manager {
    test_manager()
}

#[cfg(any(test, feature = "test_harness"))]
pub fn get_instance() -> &'static Manager {
    test_manager()
}

unsafe impl Sync for Manager {}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Manager {
    pub const ROOT_TASK_IDENTIFIER: TaskIdentifier = TaskIdentifier::new(0);

    /// Create a new task manager instance,
    /// create a root task and register current thread as the root task main thread.
    pub const fn new() -> Self {
        Manager(RwLock::new(Inner {
            tasks: BTreeMap::new(),
            identifiers: BTreeMap::new(),
            spawners: BTreeMap::new(),
        }))
    }
}
