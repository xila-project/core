// Registration module - handles task registration and basic task data

use super::*;
use crate::manager::Metadata;
use alloc::vec::Vec;
use smol_str::SmolStr;
use users::{GroupIdentifier, UserIdentifier};

impl Manager {
    pub(crate) async fn register(
        &self,
        parent: TaskIdentifier,
        name: &str,
    ) -> Result<TaskIdentifier> {
        let mut inner = self.0.write().await;

        // - Get parent task information if any (inheritance)
        let (parent_task_identifier, parent_environment_variables, user, group) = if inner
            .tasks
            .is_empty()
        {
            (
                Manager::ROOT_TASK_IDENTIFIER, // Root task is its own parent
                Vec::new(),
                UserIdentifier::ROOT,
                GroupIdentifier::ROOT,
            )
        } else if let Ok(metadata) = inner.tasks.get(&parent).ok_or(Error::InvalidTaskIdentifier) {
            (
                parent,
                metadata.environment_variables.clone(),
                metadata.user,
                metadata.group,
            )
        } else {
            (
                Manager::ROOT_TASK_IDENTIFIER, // If parent task not found, use root task
                Vec::new(),
                UserIdentifier::ROOT,
                GroupIdentifier::ROOT,
            )
        };

        // Truncate the name if it's too long
        let name = if name.len() > 23 {
            // Truncate the name to 32 characters if it's too long
            &name[..23]
        } else {
            name
        };

        let name = SmolStr::new_inline(name);

        let metadata = Metadata {
            internal_identifier: 0, // Will be set later
            name: name.clone(),
            parent: parent_task_identifier,
            user,
            group,
            environment_variables: parent_environment_variables,
            signals: SignalAccumulator::new(),
            spawner_identifier: 0, // Will be set later
        };

        let identifier = Self::find_first_available_identifier(
            &inner.tasks,
            (TaskIdentifierInner::MIN..TaskIdentifierInner::MAX).step_by(1),
        )
        .ok_or(Error::TooManyTasks)?;

        // Find the first available task identifier
        //let Expected = TaskIdentifier::new(Identifier);

        // Populate the identifier with the first available one
        if inner
            .tasks
            .insert(
                identifier, metadata, // We insert None to reserve the identifier
            )
            .is_some()
        {
            unreachable!("Task identifier already exists");
        }

        Ok(identifier)
    }

    /// Unregister task.
    ///
    /// If the task has children tasks, the root task adopts them.
    pub(crate) async fn unregister(&self, identifier: TaskIdentifier) -> Result<Metadata> {
        let mut inner = self.0.write().await;

        // Root task adopts all children of the task
        inner.tasks.iter_mut().for_each(|(_, task)| {
            if task.parent == identifier {
                task.parent = Self::ROOT_TASK_IDENTIFIER;
            }
        });

        let metadata = inner
            .tasks
            .remove(&identifier)
            .ok_or(Error::InvalidTaskIdentifier)?;

        // Remove the internal identifier of the task
        inner.identifiers.remove(&metadata.internal_identifier);

        Ok(metadata)
    }
}
