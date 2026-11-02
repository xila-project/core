// Metadata module - handles task metadata and data structures

use super::*;
use alloc::vec::Vec;
use smol_str::SmolStr;
use users::{GroupIdentifier, UserIdentifier};

/// Internal representation of a task.
pub(crate) struct Metadata {
    pub(crate) generation: u64,
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

/// Authority bound to one task registration in one manager. Copies retain that
/// registration; they never inherit the identity of a reused numeric task ID.
#[derive(Clone, Copy)]
pub struct TaskAuthority<'a> {
    pub(super) manager: &'a Manager,
    pub(super) task: TaskIdentifier,
    pub(super) generation: u64,
}

impl TaskAuthority<'_> {
    fn validate(&self, inner: &Inner) -> Result<UserIdentifier> {
        let metadata = inner
            .tasks
            .get(&self.task)
            .ok_or(Error::InvalidTaskIdentifier)?;
        if metadata.generation != self.generation {
            return Err(Error::InvalidTaskIdentifier);
        }
        Ok(metadata.user)
    }

    /// Read current credentials; exit or ID reuse revokes the authority.
    pub async fn user(&self) -> Result<UserIdentifier> {
        self.validate(&*self.manager.0.read().await)
    }

    /// Single-attempt lookup for synchronous adapters. `None` means contention.
    pub fn try_user(&self) -> Result<Option<UserIdentifier>> {
        match self.manager.0.try_read() {
            Ok(inner) => self.validate(&inner).map(Some),
            Err(_) => Ok(None),
        }
    }
}

impl Manager {
    pub async fn authority(&self, task: TaskIdentifier) -> Result<TaskAuthority<'_>> {
        let inner = self.0.read().await;
        let metadata = inner.tasks.get(&task).ok_or(Error::InvalidTaskIdentifier)?;
        Ok(TaskAuthority {
            manager: self,
            task,
            generation: metadata.generation,
        })
    }
}
