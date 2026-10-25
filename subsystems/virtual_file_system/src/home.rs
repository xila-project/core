use alloc::format;
use file_system::{Kind, PathOwned, Permissions};
use task::TaskIdentifier;
use users::UserIdentifier;

use crate::{Error, Result, VirtualFileSystem};

/// Stable home path, unaffected by account renaming or untrusted user names.
pub fn user_home(user: UserIdentifier) -> PathOwned {
    PathOwned::new(format!("/users/{}", user.as_u16())).expect("numeric user home is a valid path")
}

pub fn user_application_directory(user: UserIdentifier, application_id: &str) -> PathOwned {
    PathOwned::new(format!(
        "/users/{}/applications/{application_id}",
        user.as_u16()
    ))
    .expect("application identifier is a valid path component")
}

impl VirtualFileSystem {
    /// Lazily create the user's private application configuration, data and cache directories.
    pub async fn ensure_user_application_directories(
        &self,
        task: TaskIdentifier,
        user: UserIdentifier,
        application_id: &str,
    ) -> Result<PathOwned> {
        if application_id.is_empty()
            || matches!(application_id, "." | "..")
            || application_id.contains(['/', '\0'])
        {
            return Err(Error::InvalidPath);
        }
        let current_user = self.task_manager().get_user(task).await?;
        if current_user != user && current_user != UserIdentifier::ROOT {
            return Err(Error::PermissionDenied);
        }
        self.ensure_user_home(task, user).await?;
        let group = self.users_manager().get_user_primary_group(user).await?;
        let application = user_application_directory(user, application_id);
        let paths = [
            user_home(user)
                .append("applications")
                .ok_or(Error::InvalidPath)?,
            application.clone(),
            application
                .clone()
                .append("configuration")
                .ok_or(Error::InvalidPath)?,
            application
                .clone()
                .append("data")
                .ok_or(Error::InvalidPath)?,
            application
                .clone()
                .append("cache")
                .ok_or(Error::InvalidPath)?,
        ];

        for path in paths {
            match self.get_statistics(&path).await {
                Ok(statistics) => {
                    if statistics.kind != Kind::Directory || statistics.user != user {
                        return Err(Error::PermissionDenied);
                    }
                }
                Err(Error::FileSystem(file_system::Error::NotFound)) => {
                    self.create_directory(task, &path).await?;
                    self.set_permissions(task, &path, Permissions::USER_FULL)
                        .await?;
                    if current_user == UserIdentifier::ROOT && user != UserIdentifier::ROOT {
                        self.set_ownership(task, &path, Some(user), Some(group))
                            .await?;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        Ok(application)
    }

    /// Provision a private home before dropping login's privileges. Existing
    /// homes must belong to the expected user; their contents are preserved.
    pub async fn ensure_user_home(
        &self,
        task: TaskIdentifier,
        user: UserIdentifier,
    ) -> Result<PathOwned> {
        let path = user_home(user);
        match self.get_statistics(&path).await {
            Ok(statistics) => {
                if statistics.kind != Kind::Directory {
                    return Err(Error::NotADirectory);
                }
                if statistics.user != user {
                    return Err(Error::PermissionDenied);
                }
                self.set_permissions(task, &path, Permissions::USER_FULL)
                    .await?;
                return Ok(path);
            }
            Err(Error::FileSystem(file_system::Error::NotFound)) => {}
            Err(error) => return Err(error),
        }
        let group = self.users_manager().get_user_primary_group(user).await?;
        self.create_directory(task, &path).await?;
        self.set_permissions(task, &path, Permissions::USER_FULL)
            .await?;
        self.set_ownership(task, &path, Some(user), Some(group))
            .await?;
        Ok(path)
    }
}
