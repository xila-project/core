use alloc::{boxed::Box, vec::Vec};
use core::{future::Future, pin::Pin};

use exported_file_system::Permissions;
use file_system::{Kind, Path};
use task::TaskIdentifier;

use crate::{Directory, Error, Result, VirtualFileSystem};

pub fn ignore_already_exists_error<T>(result: Result<T>) -> Result<()> {
    match result {
        Ok(_) | Err(Error::AlreadyExists) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Create the system-owned directory tree. Repeated calls preserve contents.
pub async fn create_default_hierarchy(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    virtual_file_system
        .set_permissions(task, &Path::ROOT, Permissions::DIRECTORY_DEFAULT)
        .await?;

    let paths = [
        Path::SYSTEM,
        Path::SYSTEM_ACCOUNTS,
        Path::SYSTEM_ACCOUNT_USERS,
        Path::SYSTEM_ACCOUNT_GROUPS,
        Path::SYSTEM_EXECUTABLES,
        Path::SYSTEM_LIBRARIES,
        Path::SYSTEM_SERVICES,
        Path::CONFIGURATION,
        Path::SYSTEM_CONFIGURATION,
        Path::APPLICATION_CONFIGURATION,
        Path::SHARED_CONFIGURATION,
        Path::DEVICES,
        Path::AUDIO_DEVICES,
        Path::COMPUTE_DEVICES,
        Path::GRAPHICS_DEVICES,
        Path::INPUT_DEVICES,
        Path::NETWORK_DEVICES,
        Path::RANDOM_DEVICES,
        Path::STANDARD_IO_DEVICES,
        Path::STORAGE_DEVICES,
        Path::TIME_DEVICES,
        Path::APPLICATIONS,
        Path::APPLICATION_EXECUTABLES,
        Path::APPLICATION_LIBRARIES,
        Path::DATA,
        Path::SYSTEM_DATA,
        Path::SYSTEM_LOGS,
        Path::APPLICATION_DATA,
        Path::SHARED_DATA,
        Path::USERS,
        Path::TEMPORARY,
        Path::TEMPORARY_CACHE,
        Path::TEMPORARY_SESSIONS,
    ];

    for path in paths {
        ignore_already_exists_error(virtual_file_system.create_directory(task, &path).await)?;
    }

    virtual_file_system
        .set_permissions(task, &Path::DEVICES, Permissions::ALL_FULL)
        .await?;

    Ok(())
}

/// Remove every entry below `/temporary`, preserving the directory itself.
pub async fn clear_temporary(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    clear_directory_contents(virtual_file_system, task, Path::TEMPORARY).await
}

fn clear_directory_contents<'a>(
    virtual_file_system: &'a VirtualFileSystem,
    task: TaskIdentifier,
    path: &'a Path,
) -> Pin<Box<dyn Future<Output = Result<()>> + 'a>> {
    Box::pin(async move {
        let mut directory = Directory::open(virtual_file_system, task, path).await?;
        let mut entries = Vec::new();
        while let Some(entry) = directory.read().await? {
            entries.push(entry);
        }
        directory.close(virtual_file_system).await?;

        for entry in entries {
            let entry_path = path
                .join(Path::from_str(&entry.name))
                .ok_or(Error::InvalidPath)?;
            if entry.kind == Kind::Directory {
                clear_directory_contents(virtual_file_system, task, &entry_path).await?;
            }
            virtual_file_system.remove(task, &entry_path).await?;
        }

        Ok(())
    })
}

pub async fn clean_devices_in_directory(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: &Path,
) -> Result<()> {
    for entry in Directory::open(virtual_file_system, task, path).await? {
        if entry.kind != Kind::File {
            continue;
        }

        let entry_path = path.append(&entry.name).ok_or(Error::InvalidPath)?;
        let kind = virtual_file_system.get_statistics(&entry_path).await?.kind;

        if kind != Kind::CharacterDevice && kind != Kind::BlockDevice {
            continue;
        }

        match virtual_file_system.remove(task, &entry_path).await {
            Ok(()) | Err(Error::InvalidIdentifier) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

pub async fn clean_devices(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    clean_devices_in_directory(virtual_file_system, task, Path::DEVICES).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_already_exists_error_only_suppresses_existing_paths() {
        assert_eq!(
            ignore_already_exists_error::<()>(Err(Error::AlreadyExists)),
            Ok(())
        );
        assert_eq!(
            ignore_already_exists_error::<()>(Err(Error::PermissionDenied)),
            Err(Error::PermissionDenied)
        );
    }
}
