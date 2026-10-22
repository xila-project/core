// Relationships module - handles parent/child relationships between tasks

use alloc::vec::Vec;

use super::*;

impl Manager {
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
