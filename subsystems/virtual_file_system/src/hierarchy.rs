use alloc::{borrow::ToOwned, vec::Vec};

use exported_file_system::Permissions;
use file_system::{Kind, Path};
use task::TaskIdentifier;

use crate::{Directory, IoErrorKind, Result, VirtualFileSystem};

pub fn ignore_already_exists_error<T>(result: Result<T>) -> Result<()> {
    match result {
        Ok(_) | Err(IoErrorKind::AlreadyExists) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn set_permissions_if_different(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: &Path,
    permissions: Permissions,
) -> Result<()> {
    if virtual_file_system
        .get_statistics_for_task(task, &path)
        .await?
        .permissions
        != permissions
    {
        virtual_file_system
            .set_permissions(task, path, permissions)
            .await?;
    }

    Ok(())
}

/// Create the system-owned directory tree. Repeated calls preserve contents.
pub async fn create_default_hierarchy(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    set_permissions_if_different(
        virtual_file_system,
        task,
        Path::ROOT,
        Permissions::DIRECTORY_DEFAULT,
    )
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
        Path::PACKAGE_CONFIGURATION,
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
        Path::APPLICATION_BUNDLES,
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
        match virtual_file_system.create_directory(task, &path).await {
            Ok(()) => {}
            Err(IoErrorKind::AlreadyExists) => {
                if virtual_file_system
                    .get_statistics_for_task(task, &path)
                    .await?
                    .kind
                    != Kind::Directory
                {
                    return Err(IoErrorKind::NotADirectory);
                }
            }
            Err(error) => return Err(error),
        }
    }

    set_permissions_if_different(
        virtual_file_system,
        task,
        Path::DEVICES,
        Permissions::ALL_FULL,
    )
    .await?;

    // Without per-application identities, global application state is
    // root-only; applications keep data in <home>/applications/.
    for path in [Path::APPLICATION_DATA, Path::APPLICATION_CONFIGURATION] {
        set_permissions_if_different(virtual_file_system, task, path, Permissions::USER_FULL)
            .await?;
    }

    for path in [Path::SHARED_DATA, Path::TEMPORARY, Path::TEMPORARY_CACHE] {
        set_permissions_if_different(
            virtual_file_system,
            task,
            path,
            Permissions::SHARED_DIRECTORY,
        )
        .await?;
    }

    Ok(())
}

/// Remove every entry below `/temporary`, preserving the directory itself and
/// the subdirectories of the default hierarchy.
pub async fn clear_temporary(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
) -> Result<()> {
    // The directories `create_default_hierarchy` owns are emptied but kept:
    // removing them would recreate them, and write them, at every boot.
    clear_directory_contents(
        virtual_file_system,
        task,
        Path::TEMPORARY,
        &[
            Path::TEMPORARY_CACHE.as_str(),
            Path::TEMPORARY_SESSIONS.as_str(),
        ],
    )
    .await
}

async fn clear_directory_contents(
    virtual_file_system: &VirtualFileSystem,
    task: TaskIdentifier,
    path: &Path,
    kept: &[&str],
) -> Result<()> {
    // Post-order traversal: only keep pending siblings and ancestor removals,
    // not the entire tree or a chain of recursively polled futures.
    let mut pending = Vec::new();
    pending.push((path.to_owned(), false));

    while let Some((directory_path, remove)) = pending.pop() {
        if remove {
            virtual_file_system.remove(task, &directory_path).await?;
            continue;
        }

        let mut directory = Directory::open(virtual_file_system, task, &directory_path).await?;
        if directory_path.as_str() != path.as_str() && !kept.contains(&directory_path.as_str()) {
            pending.push((directory_path.clone(), true));
        }
        while let Some(entry) = directory.read().await? {
            // Following either entry would revisit directories or escape the root.
            if entry.name == "." || entry.name == ".." {
                continue;
            }
            let entry_path = entry
                .join_path(&directory_path)
                .ok_or(IoErrorKind::InvalidPath)?;
            pending.push((entry_path, entry.kind != Kind::Directory));
        }
        // Never mutate a directory while its entries are being enumerated.
        directory.close(virtual_file_system).await?;
    }

    Ok(())
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

        let entry_path = path.append(&entry.name).ok_or(IoErrorKind::InvalidPath)?;
        let kind = virtual_file_system.get_statistics(&entry_path).await?.kind;

        if kind != Kind::CharacterDevice && kind != Kind::BlockDevice {
            continue;
        }

        match virtual_file_system.remove(task, &entry_path).await {
            Ok(()) | Err(IoErrorKind::BadDescriptor) => {}
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
            ignore_already_exists_error::<()>(Err(IoErrorKind::AlreadyExists)),
            Ok(())
        );
        assert_eq!(
            ignore_already_exists_error::<()>(Err(IoErrorKind::PermissionDenied)),
            Err(IoErrorKind::PermissionDenied)
        );
    }
}
