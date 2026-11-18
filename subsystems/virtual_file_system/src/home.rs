use file_system::{Kind, PathOwned, Permissions};
use task::TaskIdentifier;
use users::UserIdentifier;

use crate::{Error, Result, VirtualFileSystem};

fn is_valid_application_identifier(application_id: &str) -> bool {
    !application_id.is_empty()
        && !matches!(application_id, "." | "..")
        && !application_id.contains(['/', '\0'])
}

impl VirtualFileSystem {
    /// The home recorded in the user's account.
    ///
    /// Accounts record their home when they are created (`/users/<user name>`),
    /// and accounts saved before homes were recorded keep `/users/<identifier>`.
    /// The home is read from the account, never derived from the current user
    /// name, so renaming a user does not move or lose their home. Recorded
    /// homes are always a single entry directly below `/users`.
    pub async fn get_user_home(&self, user: UserIdentifier) -> Result<PathOwned> {
        let home = self.users_manager().get_user_home(user).await?;
        if !users::is_valid_user_home(&home) {
            return Err(Error::InvalidPath);
        }
        PathOwned::new(home).ok_or(Error::InvalidPath)
    }

    /// The private directory of an application inside the user's recorded home.
    pub async fn get_user_application_directory(
        &self,
        user: UserIdentifier,
        application_id: &str,
    ) -> Result<PathOwned> {
        if !is_valid_application_identifier(application_id) {
            return Err(Error::InvalidPath);
        }
        self.get_user_home(user)
            .await?
            .append("applications")
            .and_then(|path| path.append(application_id))
            .ok_or(Error::InvalidPath)
    }

    /// Lazily create the user's private application configuration, data and cache directories.
    pub async fn ensure_user_application_directories(
        &self,
        task: TaskIdentifier,
        user: UserIdentifier,
        application_id: &str,
    ) -> Result<PathOwned> {
        if !is_valid_application_identifier(application_id) {
            return Err(Error::InvalidPath);
        }
        let current_user = self.task_manager().get_user(task).await?;
        if current_user != user && current_user != UserIdentifier::ROOT {
            return Err(Error::PermissionDenied);
        }
        let home = self.ensure_user_home(task, user).await?;
        let group = self.users_manager().get_user_primary_group(user).await?;
        let application = self
            .get_user_application_directory(user, application_id)
            .await?;
        let paths = [
            home.append("applications").ok_or(Error::InvalidPath)?,
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
        let path = self.get_user_home(user).await?;
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
