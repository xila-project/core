// Properties module - handles task properties like user, group, and environment variables

use alloc::string::ToString;

use super::*;
use alloc::{string::String, vec::Vec};
use users::{GroupIdentifier, UserIdentifier};

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

        // We remove any existing environment variable with the same name
        metadata
            .environment_variables
            .retain(|variable| variable.get_name() != name);
        // Add the new environment variable
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

    pub async fn remove_environment_variable(
        &self,
        task_identifier: TaskIdentifier,
        name: &str,
    ) -> Result<()> {
        Self::get_task_mutable(&mut *self.0.write().await, task_identifier)?
            .environment_variables
            .retain(|variable| variable.get_name() != name);

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
        Self::get_task(&*self.0.read().await, task_identifier).map(|task| task.name.to_string())
    }
}
