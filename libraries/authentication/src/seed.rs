//! First-install account setup for explicitly marked reference images.
use crate::{Context, Error, Result};
/// Marker for an explicitly provisioned reference image. Not application metadata.
pub const SEED_PENDING_PATH: &str = "/system/accounts/.seed-pending";
pub const SEED_PENDING_CONTENT: &[u8] = b"xila-seed-v1\n";
use alloc::vec::Vec;
use file_system::{AccessFlags, CreateFlags, Flags, Permissions};
use users::{GroupIdentifier, UserIdentifier};
use virtual_file_system::{Error as FsError, File};

/// Load saved users and finish an explicitly pending seed installation.
/// An unmarked disk never authorizes account creation or password replacement.
pub async fn initialize_seeded(context: &Context<'_>) -> Result<()> {
    let vfs = context.virtual_file_system;
    let task = context.task;
    let pending = match File::open(vfs, task, SEED_PENDING_PATH, AccessFlags::Read.into()).await {
        Ok(mut file) => {
            let mut data = [0; 32];
            let mut length = 0;
            loop {
                if length == data.len() {
                    return Err(invalid());
                }
                let read = file
                    .read(&mut data[length..])
                    .await
                    .map_err(Error::FailedToReadUserFile)?;
                if read == 0 {
                    break;
                }
                length += read;
            }
            file.close(vfs).await.map_err(Error::FailedToCloseFile)?;
            if &data[..length] != SEED_PENDING_CONTENT {
                return Err(invalid());
            }
            true
        }
        Err(FsError::FileSystem(file_system::Error::NotFound)) => false,
        Err(error) => return Err(Error::FailedToReadUserFile(error)),
    };
    crate::load_all_users_and_groups(context).await?;
    if !pending {
        return Ok(());
    }

    let group_id = GroupIdentifier::new(1000);
    let user_id = UserIdentifier::new(1000);
    let mut buffer = Vec::new();
    if exists(context, "/system/accounts/groups/administrator").await? {
        let group = crate::read_group_file(context, &mut buffer, "administrator").await?;
        if group.get_identifier() != group_id || group.get_name() != "administrator" {
            return Err(invalid());
        }
    } else {
        if context.users_manager.get_group_name(group_id).await.is_ok() {
            return Err(invalid());
        }
        let group = crate::Group::new(1000, "administrator".into(), Vec::new());
        atomic_write(
            context,
            "/system/accounts/.seed-group.tmp",
            "/system/accounts/groups/administrator",
            miniserde::json::to_string(&group).as_bytes(),
        )
        .await?;
        context
            .users_manager
            .add_group(group_id, "administrator", &[])
            .await
            .map_err(Error::FailedToAddGroup)?;
    }
    if exists(context, "/system/accounts/users/administrator").await? {
        let user = crate::read_user_file(context, &mut buffer, "administrator").await?;
        if user.get_identifier() != user_id
            || user.get_primary_group() != group_id
            || user.get_name() != "administrator"
        {
            return Err(invalid());
        }
    } else {
        if context.users_manager.get_user_name(user_id).await.is_ok() {
            return Err(invalid());
        }
        let salt = crate::hash::generate_salt(vfs, task).await?;
        let hash = crate::hash::hash_password(vfs, task, "", &salt).await?;
        let user = crate::User::new(1000, "administrator".into(), 1000, hash, salt);
        atomic_write(
            context,
            "/system/accounts/.seed-user.tmp",
            "/system/accounts/users/administrator",
            miniserde::json::to_string(&user).as_bytes(),
        )
        .await?;
        context
            .users_manager
            .add_user(user_id, "administrator", group_id)
            .await
            .map_err(Error::FailedToAddUser)?;
    }
    vfs.remove(task, &SEED_PENDING_PATH)
        .await
        .map_err(Error::FailedToWriteUserFile)?;
    Ok(())
}

fn invalid() -> Error {
    Error::FailedToReadUserFile(FsError::FileSystem(file_system::Error::InvalidFile))
}
async fn exists(context: &Context<'_>, path: &str) -> Result<bool> {
    match context.virtual_file_system.get_statistics(&path).await {
        Ok(_) => Ok(true),
        Err(FsError::FileSystem(file_system::Error::NotFound)) => Ok(false),
        Err(error) => Err(Error::FailedToReadUserFile(error)),
    }
}
async fn atomic_write(
    context: &Context<'_>,
    temporary: &str,
    destination: &str,
    contents: &[u8],
) -> Result<()> {
    let vfs = context.virtual_file_system;
    let mut file = File::open(
        vfs,
        context.task,
        temporary,
        Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
    )
    .await
    .map_err(Error::FailedToWriteUserFile)?;
    file.set_permissions(Permissions::USER_READ_WRITE)
        .await
        .map_err(Error::FailedToWriteUserFile)?;
    let mut offset = 0;
    while offset < contents.len() {
        let count = file
            .write(&contents[offset..])
            .await
            .map_err(Error::FailedToWriteUserFile)?;
        if count == 0 {
            return Err(invalid());
        }
        offset += count;
    }
    file.flush().await.map_err(Error::FailedToWriteUserFile)?;
    file.close(vfs).await.map_err(Error::FailedToCloseFile)?;
    vfs.rename(context.task, &temporary, &destination)
        .await
        .map_err(Error::FailedToWriteUserFile)
}
