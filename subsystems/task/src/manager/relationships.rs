// Relationships module - handles parent/child relationships between tasks

use alloc::vec::Vec;

use super::*;

impl Manager {
    /// Number of currently registered tasks, including the root task.
    ///
    /// This is an instantaneous registry count, not a CPU utilization measure.
    /// It does not allocate or traverse task relationships.
    pub async fn get_task_count(&self) -> usize {
        self.0.read().await.tasks.len()
    }

    /// Get the children tasks of a task.
    pub async fn get_children(
        &self,
        task_identifier: TaskIdentifier,
    ) -> Result<Vec<TaskIdentifier>> {
        Ok(self
            .0
            .read()
            .await
            .tasks
            .iter()
            .filter(|(_, metadata)| metadata.parent == task_identifier)
            .map(|(identifier, _)| *identifier)
            .collect())
    }

    /// Get the parent task of a task.
    pub async fn get_parent(&self, task_identifier: TaskIdentifier) -> Result<TaskIdentifier> {
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.parent)
    }
}
