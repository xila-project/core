// Metadata module - handles task metadata and data structures

use super::*;
use alloc::vec::Vec;
use smol_str::SmolStr;
use users::{GroupIdentifier, UserIdentifier};

/// Internal representation of a task.
pub(crate) struct Metadata {
    /// Internal identifier of the task.
    pub(crate) internal_identifier: usize,
    /// Name of the task.
    pub(crate) name: SmolStr,
    /// The children of the task.
    pub(crate) parent: TaskIdentifier,
    /// The identifier of the user that owns the task.
    pub(crate) user: UserIdentifier,
    /// The identifier of the group that owns the task.
    pub(crate) group: GroupIdentifier,
    /// Environment variables of the task.
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    /// Signals
    pub(crate) signals: SignalAccumulator,
    /// Index of the spawner that spawned this task (for tracking completion)
    pub(crate) spawner_identifier: usize,
}
