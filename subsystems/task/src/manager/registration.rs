// Registration module - handles task registration and basic task data

use super::*;
use crate::manager::Metadata;
use alloc::vec::Vec;
use smol_str::SmolStr;
use users::{GroupIdentifier, UserIdentifier};

impl Manager {
    #[cfg(test)]
    pub(crate) async fn register(
        &self,
        parent: TaskIdentifier,
        name: &str,
    ) -> Result<TaskIdentifier> {
        let mut inner = self.0.write().await;
        Self::register_inner(&mut inner, parent, name)
    }

    pub(super) fn register_inner(
        inner: &mut Inner,
        parent: TaskIdentifier,
        name: &str,
    ) -> Result<TaskIdentifier> {
        if inner.tasks.len() >= inner.maximum_tasks {
            return Err(Error::TooManyTasks);
        }
        // - Get parent task information if any (inheritance)
        let (parent_task_identifier, parent_environment_variables, user, group) = if inner
            .tasks
            .is_empty()
        {
            if inner.root_established || parent != Self::ROOT_TASK_IDENTIFIER {
                return Err(Error::InvalidTaskIdentifier);
            }
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
            return Err(Error::InvalidTaskIdentifier);
        };

        // SmolStr's inline capacity is 23 bytes, not characters.
        let mut end = name.len().min(23);
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        let name = &name[..end];

        let name = SmolStr::new_inline(name);

        let generation = inner
            .next_generation
            .checked_add(1)
            .ok_or(Error::TooManyTasks)?;
        inner.next_generation = generation;
        let metadata = Metadata {
            generation,
            internal_identifier: 0, // Will be set later
            name: name.clone(),
            parent: parent_task_identifier,
            user,
            group,
            environment_variables: parent_environment_variables,
            signals: SignalAccumulator::new(),
            spawner_identifier: usize::MAX, // No spawner until enqueued
        };

        let mut candidate = TaskIdentifierInner::from(inner.root_established);
        for identifier in inner.tasks.keys() {
            let used = identifier.into_inner();
            if used < candidate {
                continue;
            }
            if used > candidate {
                break;
            }
            candidate = candidate.checked_add(1).ok_or(Error::TooManyTasks)?;
        }
        let identifier = TaskIdentifier::new(candidate);

        // Numeric zero remains reserved after root exit.
        inner.tasks.insert(identifier, metadata);

        inner.root_established = true;
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
        if inner.identifiers.get(&metadata.internal_identifier) == Some(&identifier) {
            inner.identifiers.remove(&metadata.internal_identifier);
            self.1.lock(|spawners| {
                if let Some(spawner) = spawners.borrow_mut().get_mut(&metadata.spawner_identifier) {
                    spawner.tasks -= 1;
                }
            });
        }

        Ok(metadata)
    }
}
