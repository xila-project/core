#![no_std]

extern crate alloc;

embedded_i18n::track_locales!();

#[cfg(feature = "building")]
mod building;
mod error;
mod standard;
mod traits;

#[cfg(feature = "building")]
pub use building::*;
pub use error::*;
pub use file_system as exported_file_system;
pub use standard::*;
pub use task as exported_task;
pub use traits::*;
pub use virtual_file_system as exported_virtual_file_system;

use alloc::{string::String, vec::Vec};
use file_system::{AccessFlags, Path, Permission, Statistics};
use task::{JoinHandle, SpawnerIdentifier, TaskIdentifier};
use users::UserIdentifier;
use virtual_file_system::File;

/// Check execute permissions for a user using the same owner, group and root
/// rules as [`execute`]. This checks permissions only; it does not open the file
/// or validate its executable format.
pub async fn is_execute_allowed(
    users_manager: &users::Manager,
    statistics: &Statistics,
    user: UserIdentifier,
) -> bool {
    if statistics
        .permissions
        .get_others()
        .contains(Permission::Execute)
    {
        return true;
    }

    let is_user_allowed = user == UserIdentifier::ROOT || user == statistics.user;
    if is_user_allowed
        && statistics
            .permissions
            .get_user()
            .contains(Permission::Execute)
    {
        return true;
    }

    let is_in_group =
        users_manager.is_in_group(user, statistics.group).await || user == UserIdentifier::ROOT;
    if (is_in_group)
        && statistics
            .permissions
            .get_group()
            .contains(Permission::Execute)
    {
        return true;
    }

    false
}

async fn get_overridden_user(
    context: &ExecutableContext,
    statistics: &Statistics,
    task: TaskIdentifier,
) -> Result<Option<UserIdentifier>> {
    if !statistics
        .permissions
        .get_special()
        .get_set_user_identifier()
    {
        return Ok(None);
    }

    let current_user = context.task_manager.get_user(task).await?;

    let new_user = statistics.user;

    if current_user != users::UserIdentifier::ROOT || new_user != current_user {
        return Err(Error::PermissionDenied);
    }

    Ok(Some(new_user))
}

pub async fn execute(
    context: &'static ExecutableContext,
    path: impl AsRef<Path>,
    inputs: Vec<String>,
    standard: Standard<'static>,
    spawner: Option<SpawnerIdentifier>,
) -> Result<JoinHandle<isize>> {
    execute_with_environment(context, path, inputs, standard, spawner, Vec::new()).await
}

fn validate_environment(environment: &[(String, String)]) -> Result<()> {
    if environment
        .iter()
        .any(|(name, value)| name.is_empty() || name.contains(['=', '\0']) || value.contains('\0'))
    {
        return Err(task::Error::InvalidEnvironmentVariable.into());
    }
    Ok(())
}

async fn prepare_launch_environment(
    tasks: &task::Manager,
    identifier: task::TaskIdentifier,
    environment: &[(String, String)],
) -> task::Result<()> {
    // Child tasks inherit environment values, but a new executable has its own
    // identity and must not accidentally inherit the launching app's icon.
    #[cfg(feature = "graphics")]
    tasks
        .remove_environment_variable(identifier, graphics::ICON_ENVIRONMENT)
        .await?;
    for (name, value) in environment {
        tasks
            .set_environment_variable(identifier, name, value)
            .await?;
    }
    Ok(())
}

/// Execute with caller-provided environment overrides installed before
/// the new program is polled. The overrides are private to the spawned task and
/// inherited by its children.
pub async fn execute_with_environment(
    context: &'static ExecutableContext,
    path: impl AsRef<Path>,
    inputs: Vec<String>,
    standard: Standard<'static>,
    spawner: Option<SpawnerIdentifier>,
    environment: Vec<(String, String)>,
) -> Result<JoinHandle<isize>> {
    validate_environment(&environment)?;
    let task_instance = &context.task_manager;

    let task = task_instance.get_current_task_identifier().await;

    let virtual_file_system = &context.virtual_file_system;

    let statistics = virtual_file_system.get_statistics(&path.as_ref()).await?;

    // - Check the executable bit
    if !is_execute_allowed(
        context.users_manager,
        &statistics,
        task_instance.get_user(task).await?,
    )
    .await
    {
        return Err(Error::PermissionDenied);
    }

    let mut file = File::open(virtual_file_system, task, &path, AccessFlags::Read.into()).await?;

    // - Check if the user can override the user identifier
    let new_user = get_overridden_user(context, &statistics, task).await?;

    let application_name = application_metadata::application_name_for_path(path.as_ref().as_str());
    let application_identifier =
        application_metadata::application_identifier_for_path(path.as_ref().as_str());
    if let Some(application_name) = application_name {
        let user = match new_user {
            Some(user) => user,
            None => context.task_manager.get_user(task).await?,
        };
        virtual_file_system
            .ensure_user_application_directories(task, user, application_name)
            .await?;
    }

    let file_name = path
        .as_ref()
        .get_file_name()
        .ok_or(virtual_file_system::Error::InvalidPath)?;

    let main_function: MainFunction = file.control(GET_MAIN_FUNCTION, &()).await?;

    let main = main_function.ok_or(Error::FailedToGetMainFunction)?;

    let (join_handle, _) = task_instance
        .spawn_with_application_identifier(
            task,
            file_name,
            spawner,
            application_identifier,
            async move |task| {
                if prepare_launch_environment(context.task_manager, task, &environment)
                    .await
                    .is_err()
                {
                    return -1;
                }
                drop(environment);
                if let Some(new_user) = new_user {
                    context.task_manager.set_user(task, new_user).await.unwrap();
                }

                match main(context, standard, inputs).await {
                    Ok(_) => 0_isize,
                    Err(error) => -(error.get() as isize),
                }
            },
        )
        .await?;

    Ok(join_handle)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use file_system::{Permissions, Time};

    use task::test;
    use users::GroupIdentifier;

    use super::*;

    #[cfg(feature = "graphics")]
    #[test]
    async fn executable_launch_clears_inherited_icon_without_an_override() {
        let manager = task::get_instance();
        let current = manager.get_current_task_identifier().await;
        manager
            .set_environment_variable(current, graphics::ICON_ENVIRONMENT, "parent-icon")
            .await
            .unwrap();
        prepare_launch_environment(manager, current, &[])
            .await
            .unwrap();
        assert!(
            manager
                .get_environment_variable(current, graphics::ICON_ENVIRONMENT)
                .await
                .is_err()
        );
        prepare_launch_environment(
            manager,
            current,
            &[(graphics::ICON_ENVIRONMENT.into(), "child-icon".into())],
        )
        .await
        .unwrap();
        assert_eq!(
            manager
                .get_environment_variable(current, graphics::ICON_ENVIRONMENT)
                .await
                .unwrap()
                .get_value(),
            "child-icon"
        );
    }

    fn get_statistics_with_permissions(permissions: Permissions) -> Statistics {
        Statistics::new(
            0,
            1,
            0,
            Time::new(0),
            Time::new(0),
            Time::new(0),
            Time::new(0),
            file_system::Kind::File,
            permissions,
            UserIdentifier::ROOT,
            GroupIdentifier::ROOT,
        )
    }

    #[test]
    async fn test_is_execute_allowed() {
        let users_manager = users::Manager::new();

        let statistics = get_statistics_with_permissions(Permissions::ALL_FULL);
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::EXECUTABLE);
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::from_octal(0o007).unwrap());
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::from_octal(0o070).unwrap());
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::from_octal(0o100).unwrap());
        assert!(is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::USER_READ_WRITE);
        assert!(!is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::NONE);
        assert!(!is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);

        let statistics = get_statistics_with_permissions(Permissions::ALL_READ_WRITE);
        assert!(!is_execute_allowed(&users_manager, &statistics, UserIdentifier::ROOT).await);
    }
}
