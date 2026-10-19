use alloc::format;
use file_system::{Kind, PathOwned, Permissions};
use task::TaskIdentifier;
use users::UserIdentifier;

use crate::{Error, Result, VirtualFileSystem};

/// Stable home path, unaffected by account renaming or untrusted user names.
pub fn user_home(user: UserIdentifier) -> PathOwned {
    PathOwned::new(format!("/users/{}", user.as_u16())).expect("numeric user home is a valid path")
}

impl VirtualFileSystem {
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
