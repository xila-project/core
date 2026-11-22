// Properties module - handles task properties like user, group, and environment variables

use super::*;
use alloc::{string::String, vec::Vec};
use users::{GroupIdentifier, UserIdentifier};

/// Remove the variables called `name`. One non-generic copy serves set, remove and
/// replace.
fn remove_named(variables: &mut Vec<EnvironmentVariable>, name: &str) {
    variables.retain(|variable| variable.get_name() != name);
}

impl Manager {
    pub async fn set_user(
        &self,
        task_identifier: TaskIdentifier,
        user: UserIdentifier,
    ) -> Result<()> {
        Self::get_task_mutable(&mut *self.0.write().await, task_identifier)?.user = user;

        Ok(())
    }

    pub async fn set_group(
        &self,
        task_identifier: TaskIdentifier,
        group: GroupIdentifier,
    ) -> Result<()> {
        Self::get_task_mutable(&mut *self.0.write().await, task_identifier)?.group = group;

        Ok(())
    }

    /// Give `task_identifier` the identity `user` and `group` on behalf of the
    /// program that launched it, instead of the one it inherited.
    ///
    /// The task, and every task it spawns from then on, is marked as running
    /// with a granted identity (see [`Self::is_identity_granted`]). The mark is
    /// set together with the identity, under one lock, and nothing clears it:
    /// neither a later identity change nor the exit of an ancestor, so a
    /// descendant keeps it even once it has been reparented to the root task.
    pub async fn grant_identity(
        &self,
        task_identifier: TaskIdentifier,
        user: UserIdentifier,
        group: GroupIdentifier,
    ) -> Result<()> {
        let mut inner = self.0.write().await;
        let metadata = Self::get_task_mutable(&mut inner, task_identifier)?;
        metadata.user = user;
        metadata.group = group;
        metadata.identity_granted = true;

        Ok(())
    }

    /// Whether `task_identifier`, or one of its ancestors, was given its
    /// identity by its launcher through [`Self::grant_identity`]. Runtimes for
    /// untrusted code use it to refuse running with an identity that another
    /// program granted.
    pub async fn is_identity_granted(&self, task_identifier: TaskIdentifier) -> Result<bool> {
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.identity_granted)
    }

    pub async fn set_environment_variable(
        &self,
        task_identifier: TaskIdentifier,
        name: &str,
        value: &str,
    ) -> Result<()> {
        let environment_variable = EnvironmentVariable::new(name, value);

        // Keep the write lock for the entire operation
        let mut inner = self.0.write().await;
        let metadata = Self::get_task_mutable(&mut inner, task_identifier)?;

        // A variable of the same name is replaced.
        remove_named(&mut metadata.environment_variables, name);
        metadata.environment_variables.push(environment_variable);

        Ok(())
    }

    pub async fn set_environment_variables(
        &self,
        task_identifier: TaskIdentifier,
        environment_variables: &[(&str, &str)],
    ) -> Result<()> {
        let mut inner = self.0.write().await;
        let metadata = Self::get_task_mutable(&mut inner, task_identifier)?;

        environment_variables.iter().for_each(|(name, value)| {
            let environment_variable = EnvironmentVariable::new(name, value);
            metadata.environment_variables.push(environment_variable);
        });

        Ok(())
    }

    /// Replace the whole environment of a task with `variables`, as one atomic
    /// update. A later entry overrides an earlier one of the same name. A spawned
    /// task starts with a copy of its parent's environment; this gives it an
    /// explicit one instead.
    pub async fn replace_environment_variables(
        &self,
        task_identifier: TaskIdentifier,
        variables: Vec<EnvironmentVariable>,
    ) -> Result<()> {
        // Deduplicate before taking the lock, so the lock covers no allocation
        // beyond the replaced list's release.
        let mut replacement: Vec<EnvironmentVariable> = Vec::with_capacity(variables.len());
        for variable in variables {
            remove_named(&mut replacement, variable.get_name());
            replacement.push(variable);
        }
        let mut inner = self.0.write().await;
        Self::get_task_mutable(&mut inner, task_identifier)?.environment_variables = replacement;

        Ok(())
    }

    pub async fn remove_environment_variable(
        &self,
        task_identifier: TaskIdentifier,
        name: &str,
    ) -> Result<()> {
        remove_named(
            &mut Self::get_task_mutable(&mut *self.0.write().await, task_identifier)?
                .environment_variables,
            name,
        );

        Ok(())
    }

    /// Get user identifier of the owner of a task.
    pub async fn get_user(&self, task_identifier: TaskIdentifier) -> Result<UserIdentifier> {
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.user)
    }

    /// Get group identifier of the owner of a task.
    pub async fn get_group(&self, task_identifier: TaskIdentifier) -> Result<GroupIdentifier> {
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.group)
    }

    pub async fn get_environment_variable(
        &self,
        task_identifier: TaskIdentifier,
        name: &str,
    ) -> Result<EnvironmentVariable> {
        Self::get_task(&*self.0.read().await, task_identifier)?
            .environment_variables
            .iter()
            .find(|variable| variable.get_name() == name)
            .cloned()
            // If the variable is not found, return an error
            .ok_or(Error::InvalidEnvironmentVariable)
    }

    /// Get one environment entry by its current index, sharing its string.
    pub async fn get_environment_variable_at(
        &self,
        task_identifier: TaskIdentifier,
        index: usize,
    ) -> Result<Option<EnvironmentVariable>> {
        Ok(Self::get_task(&*self.0.read().await, task_identifier)?
            .environment_variables
            .get(index)
            .cloned())
    }

    pub async fn get_environment_variables(
        &self,
        task_identifier: TaskIdentifier,
    ) -> Result<Vec<EnvironmentVariable>> {
        Self::get_task(&*self.0.read().await, task_identifier)
            .map(|task| task.environment_variables.clone())
            .map_err(|_| Error::InvalidTaskIdentifier)
    }

    /// # Arguments
    /// * `Task_identifier` - The identifier of the task.
    pub async fn get_name(&self, task_identifier: TaskIdentifier) -> Result<String> {
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.name.clone())
    }
}
