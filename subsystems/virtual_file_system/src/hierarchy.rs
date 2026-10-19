use exported_file_system::Permissions;
/// Hierarchy of the file system.
use file_system::{Kind, Path};
use task::TaskIdentifier;

use crate::{Directory, Error, Result, VirtualFileSystem};

pub fn ignore_already_exists_error<T>(result: Result<T>) -> Result<()> {
    match result {
        Ok(_) | Err(Error::AlreadyExists) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Create the default hierarchy of the file system.
pub async fn create_default_hierarchy(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    virtual_file_system
        .set_permissions(task, &Path::ROOT, Permissions::DIRECTORY_DEFAULT)
        .await?;

    let paths = [
        Path::SYSTEM,
        Path::CONFIGURATION,
        Path::SHARED_CONFIGURATION,
        Path::DEVICES,
        Path::USERS,
        Path::DATA,
        Path::SHARED_DATA,
        Path::BINARIES,
        Path::TEMPORARY,
        Path::LOGS,
        Path::NETWORK_DEVICES,
    ];

    for path in paths {
        ignore_already_exists_error(virtual_file_system.create_directory(task, &path).await)?;
    }

    virtual_file_system
        .set_permissions(task, &Path::DEVICES, Permissions::ALL_FULL)
        .await?;

    Ok(())
}

pub async fn clean_devices_in_directory(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: &Path,
) -> Result<()> {
    // For each entry in the directory.
    for entry in Directory::open(virtual_file_system, task, path).await? {
        if entry.kind != Kind::File {
            continue;
        }

        let entry_path = path.append(&entry.name).unwrap();

        let kind = virtual_file_system.get_statistics(&entry_path).await?.kind;

        if kind != Kind::CharacterDevice && kind != Kind::BlockDevice {
            continue;
        }

        match virtual_file_system.remove(task, &entry_path).await {
            Ok(_) | Err(Error::InvalidIdentifier) => {}
            Err(error) => {
                return Err(error);
            }
        }
    }

    Ok(())
}

pub async fn clean_devices(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    clean_devices_in_directory(virtual_file_system, task, Path::DEVICES).await?;

    clean_devices_in_directory(virtual_file_system, task, Path::BINARIES).await?;

    Ok(())
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
