use crate::{Error, Result, VirtualFileSystem, pipe::Pipe, poll};
use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use exported_file_system::{
    AttributeFlags, Attributes, BlockDevice, CharacterDevice, FileSystemOperations, Inode, Path,
    PathOwned, Permission, Permissions, Time,
};
use task::TaskIdentifier;
use users::{GroupIdentifier, UserIdentifier};

pub(super) struct InternalFileSystem {
    pub reference_count: usize,
    /// Whether this filesystem participates in path resolution. Inactive
    /// entries remain here while open contexts still hold its static handle.
    pub is_mounted: bool,
    pub mount_point: PathOwned,
    pub file_system: &'static dyn FileSystemOperations,
}

pub(super) struct InternalBlockDevice {
    pub reference_count: usize,
    pub device: &'static dyn BlockDevice,
}

pub(super) struct InternalCharacterDevice {
    pub reference_count: usize,
    pub device: &'static dyn CharacterDevice,
}

pub(super) struct InternalPipe {
    pub pipe: synchronization::Arc<Pipe>,
}

pub(super) type FileSystemsArray = Vec<InternalFileSystem>;
pub(super) type CharacterDevicesMap = BTreeMap<Inode, InternalCharacterDevice>;
pub(super) type BlockDevicesMap = BTreeMap<Inode, InternalBlockDevice>;
pub(super) type PipeMap = BTreeMap<Inode, InternalPipe>;

impl VirtualFileSystem {
    pub(super) async fn has_permissions(
        users_manager: &users::Manager,
        current_user: UserIdentifier,
        necessary_permissions: Permission,
        owner_user: UserIdentifier,
        owner_group: GroupIdentifier,
        permissions: Permissions,
    ) -> bool {
        if current_user == UserIdentifier::ROOT {
            return true;
        }

        if current_user == owner_user && permissions.get_user().contains(necessary_permissions) {
            return true;
        }

        if users_manager.is_in_group(current_user, owner_group).await
            && permissions.get_group().contains(necessary_permissions)
        {
            return true;
        }

        if permissions.get_others().contains(necessary_permissions) {
            return true;
        }

        false
    }

    pub(super) fn get_file_system_index_from_path(
        file_systems: &FileSystemsArray,
        path: &impl AsRef<Path>,
    ) -> usize {
        let mut result_score = 0;
        let mut result: usize = 0;

        let path = path.as_ref();
        let path_components = path.get_components();

        for (i, file_system) in file_systems.iter().enumerate() {
            if !file_system.is_mounted {
                continue;
            }
            let mount_point: &Path = file_system.mount_point.as_ref();
            let mount_point_components = mount_point.get_components();

            let striped_components = path_components
                .clone()
                .strip_prefix(&mount_point_components);

            if striped_components.is_none() {
                continue;
            }

            let score = mount_point_components.count();

            // Only consider this file system if all mount point components match
            if result_score < score {
                result_score = score;
                result = i;
            }
        }

        result
    }

    pub(super) fn get_mutable_file_system_from_path<'b, 'c>(
        file_systems: &'b mut FileSystemsArray,
        path: &'c impl AsRef<Path>,
    ) -> Result<(&'b mut InternalFileSystem, &'c Path, usize)> {
        let i = Self::get_file_system_index_from_path(file_systems, path);

        let internal_file_system = &mut file_systems[i];

        let relative_path = path
            .as_ref()
            .strip_prefix_absolute(&internal_file_system.mount_point)
            .ok_or_else(|| {
                log::error!(
                    "Error stripping prefix {:?} from path {:?}",
                    internal_file_system.mount_point,
                    path.as_ref()
                );

                Error::InvalidPath
            })?;

        Ok((internal_file_system, relative_path, i))
    }

    pub(super) fn get_file_system_from_path<'b, 'c>(
        file_systems: &'b FileSystemsArray,
        path: &'c impl AsRef<Path>,
    ) -> Result<(&'b InternalFileSystem, &'c Path, usize)> {
        let i = Self::get_file_system_index_from_path(file_systems, path);

        let internal_file_system = &file_systems[i];

        let relative_path = path
            .as_ref()
            .strip_prefix_absolute(&internal_file_system.mount_point)
            .ok_or(Error::InvalidPath)?;

        Ok((internal_file_system, relative_path, i))
    }

    pub(super) async fn get_time_user_group(
        &self,
        task: TaskIdentifier,
    ) -> Result<(Time, UserIdentifier, GroupIdentifier)> {
        let time: Time = self.time_manager.get_current_time()?.into();

        let user = self.task_manager.get_user(task).await?;

        let group = self.users_manager.get_user_primary_group(user).await?;

        Ok((time, user, group))
    }

    pub(super) fn get_new_inode<T>(devices: &BTreeMap<Inode, T>) -> Option<Inode> {
        (1..Inode::MAX).find(|&inode| !devices.contains_key(&inode))
    }

    pub(super) async fn set_attributes(
        file_system: &dyn FileSystemOperations,
        path: impl AsRef<Path>,
        attributes: &Attributes,
    ) -> Result<()> {
        poll(|| {
            Ok(FileSystemOperations::set_attributes(
                file_system,
                path.as_ref(),
                attributes,
            )?)
        })
        .await
    }

    pub(super) async fn get_attributes(
        file_system: &dyn FileSystemOperations,
        path: impl AsRef<Path>,
        attributes: &mut Attributes,
    ) -> Result<()> {
        poll(|| {
            Ok(FileSystemOperations::get_attributes(
                file_system,
                path.as_ref(),
                attributes,
            )?)
        })
        .await
    }

    pub(super) async fn check_permissions(
        &self,
        file_system: &dyn FileSystemOperations,
        path: impl AsRef<Path>,
        asked_permissions: Permission,
        current_user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<()> {
        if current_user != UserIdentifier::ROOT {
            let task_application = self.task_manager.get_application_identifier(task).await?;
            if application_scope(path.as_ref()).is_some() {
                if !application_access_allowed(path.as_ref(), task_application, asked_permissions)
                {
                    return Err(Error::PermissionDenied);
                }
                // Application identity is the ownership boundary for global app trees;
                // matching apps share access across user sessions.
                return Ok(());
            }
        }

        let mut attributes = Attributes::default()
            .set_mask(AttributeFlags::User | AttributeFlags::Group | AttributeFlags::Permissions);

        Self::get_attributes(file_system, path.as_ref(), &mut attributes).await?;

        let owner_user = *attributes.get_user().ok_or(Error::MissingAttribute)?;
        let owner_group = *attributes.get_group().ok_or(Error::MissingAttribute)?;
        let owner_permissions = *attributes
            .get_permissions()
            .ok_or(Error::MissingAttribute)?;

        if !Self::has_permissions(
            self.users_manager,
            current_user,
            asked_permissions,
            owner_user,
            owner_group,
            owner_permissions,
        )
        .await
        {
            log::error!(
                "Asked permissions {:?} not granted for user {:?} on path {:?} (owner: {:?}, group: {:?}, permissions: {:?})",
                asked_permissions,
                owner_user,
                path.as_ref(),
                owner_user,
                owner_group,
                owner_permissions,
            );
            return Err(Error::PermissionDenied);
        }

        Ok(())
    }

    pub(super) async fn check_permissions_with_parent(
        &self,
        file_systems: &FileSystemsArray,
        path: impl AsRef<Path>,
        current_permission: Permission,
        parent_permission: Permission,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<()> {
        if !path.as_ref().is_root() {
            let parent_path = path.as_ref().go_parent().ok_or(Error::InvalidPath)?;

            let (parent_file_system, relative_path, _) =
                Self::get_file_system_from_path(file_systems, &parent_path)?; // Get the file system identifier and the relative path

            self.check_permissions(
                parent_file_system.file_system,
                relative_path,
                parent_permission,
                user,
                task,
            )
            .await?;
        }

        let (file_system, relative_path, _) = Self::get_file_system_from_path(file_systems, &path)?; // Get the file system identifier and the relative path

        self.check_permissions(
            file_system.file_system,
            relative_path,
            current_permission,
            user,
            task,
        )
        .await?;

        Ok(())
    }

    pub(super) async fn check_owner(
        file_system: &dyn FileSystemOperations,
        path: impl AsRef<Path>,
        current_user: UserIdentifier,
    ) -> Result<()> {
        if current_user == UserIdentifier::ROOT {
            return Ok(());
        }

        let mut attributes = Attributes::default().set_mask(AttributeFlags::User);

        Self::get_attributes(file_system, path.as_ref(), &mut attributes).await?;

        let owner_user = *attributes.get_user().ok_or(Error::MissingAttribute)?;

        if current_user == owner_user {
            return Ok(());
        }

        log::error!(
            "User {:?} is not the owner {:?} of path {:?}",
            current_user,
            owner_user,
            path.as_ref(),
        );
        Err(Error::PermissionDenied)
    }
}

fn application_scope(path: &Path) -> Option<(Option<&str>, bool)> {
    for (prefix, is_configuration) in [
        ("/data/applications", false),
        ("/configuration/applications", true),
    ] {
        if let Some(remainder) = path.as_str().strip_prefix(prefix) {
            if !remainder.is_empty() && !remainder.starts_with('/') {
                continue;
            }
            let remainder = remainder.strip_prefix('/').unwrap_or(remainder);
            let name = remainder.split('/').next()?;
            return Some(((!name.is_empty()).then_some(name), is_configuration));
        }
    }
    None
}

pub(super) fn application_access_allowed(
    path: &Path,
    task_application: Option<task::ApplicationIdentifier>,
    asked_permissions: Permission,
) -> bool {
    let Some((application, is_configuration)) = application_scope(path) else {
        return true;
    };
    let Some(application) = application else {
        // Permit traversal through the common parent, but never listing or mutation.
        return task_application.is_some() && asked_permissions == Permission::Execute;
    };
    let matching_application = application_metadata::application_identifier(application)
        .is_some_and(|expected| task_application == Some(expected));
    matching_application && !(is_configuration && asked_permissions.contains(Permission::Write))
}

#[cfg(test)]
mod application_scope_tests {
    use super::*;

    #[test]
    fn application_scope_uses_exact_component_names() {
        let clock = task::ApplicationIdentifier::new(2);
        assert!(application_access_allowed(
            Path::from_str("/data/applications/clock/settings"),
            clock,
            Permission::Read,
        ));
        assert!(!application_access_allowed(
            Path::from_str("/data/applications/clockwork/settings"),
            clock,
            Permission::Read,
        ));
        assert!(!application_access_allowed(
            Path::from_str("/data/applications/clock/settings"),
            None,
            Permission::Read,
        ));
    }

    #[test]
    fn global_application_configuration_is_read_only_to_its_application() {
        let clock = task::ApplicationIdentifier::new(2);
        assert!(application_access_allowed(
            Path::from_str("/configuration/applications/clock/preferences"),
            clock,
            Permission::Read,
        ));
        assert!(!application_access_allowed(
            Path::from_str("/configuration/applications/clock/preferences"),
            clock,
            Permission::Write,
        ));
        assert!(!application_access_allowed(
            Path::from_str("/configuration/applications/clock/preferences"),
            None,
            Permission::Read,
        ));
    }
}

#[cfg(test)]
mod tests {

    use alloc::{boxed::Box, string::ToString, vec};
    use file_system::{DummyFileSystem, PathOwned};

    use super::*;

    #[test]
    fn test_get_file_system_from_path() {
        let file_systems: FileSystemsArray = vec![
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/".to_string()).unwrap(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/Foo".to_string()).unwrap(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/Foo/Bar".to_string()).unwrap(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
        ];

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, &"/").unwrap();

        assert_eq!(i, 0);
        assert_eq!(relative_path, Path::ROOT);

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, &"/Foo/Bar").unwrap();

        assert_eq!(i, 2);
        assert_eq!(relative_path, Path::ROOT);

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, &"/Foo/Bar/Baz").unwrap();

        assert_eq!(i, 2);
        assert_eq!(relative_path, "/Baz".as_ref());

        let (_, relative_path, i) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, &"/Foo").unwrap();

        assert_eq!(i, 1);
        assert_eq!(relative_path, Path::ROOT);
    }

    #[test]
    fn path_resolution_skips_detached_mounts_after_remount() {
        let old = Box::leak(Box::new(DummyFileSystem));
        let replacement = Box::leak(Box::new(DummyFileSystem));
        let file_systems: FileSystemsArray = vec![
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/".to_string()).unwrap(),
                file_system: Box::leak(Box::new(DummyFileSystem)),
            },
            InternalFileSystem {
                reference_count: 2,
                is_mounted: false,
                mount_point: PathOwned::new("/Foo".to_string()).unwrap(),
                file_system: old,
            },
            InternalFileSystem {
                reference_count: 1,
                is_mounted: true,
                mount_point: PathOwned::new("/Foo".to_string()).unwrap(),
                file_system: replacement,
            },
        ];

        let (filesystem, relative_path, index) =
            VirtualFileSystem::get_file_system_from_path(&file_systems, &"/Foo/app").unwrap();
        assert_eq!(index, 2);
        assert!(core::ptr::eq(filesystem.file_system, replacement));
        assert_eq!(relative_path.as_str(), "/app");
    }
}
