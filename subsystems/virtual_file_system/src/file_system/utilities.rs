use crate::{Error, Result, VirtualFileSystem, pipe::Pipe, poll};
use alloc::{borrow::ToOwned, collections::btree_map::BTreeMap, string::String, vec::Vec};
use exported_file_system::{
    AccessFlags, AttributeFlags, Attributes, BlockDevice, CharacterDevice, Context,
    FileSystemOperations, Flags, Inode, Kind, Path, PathOwned, Permission, Permissions, Time,
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

pub(super) enum ResolvedPath<'a> {
    Borrowed(&'a Path),
    Owned(PathOwned),
}

struct ResolutionState {
    resolved: PathOwned,
    pending: String,
    links_followed: usize,
    target_buffer: Vec<u8>,
}

impl core::ops::Deref for ResolvedPath<'_> {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(path) => path,
            Self::Owned(path) => path,
        }
    }
}

impl AsRef<Path> for ResolvedPath<'_> {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl VirtualFileSystem {
    /// Read persisted link payloads through ordinary file operations, falling
    /// back to a backend adapter only for native host links.
    pub(super) async fn read_link_payload(
        file_system: &dyn FileSystemOperations,
        path: &Path,
        buffer: &mut Vec<u8>,
    ) -> Result<usize> {
        const MAX_TARGET_LENGTH: usize = 4096;
        let mut attributes = Attributes::new().set_mask(AttributeFlags::Size);
        Self::get_attributes(file_system, path, &mut attributes).await?;
        let size = *attributes.get_size().ok_or(Error::MissingAttribute)? as usize;
        if size == 0 || size > MAX_TARGET_LENGTH {
            return Err(Error::FileSystem(file_system::Error::InvalidSymbolicLink));
        }

        buffer.resize(size, 0);
        match poll(|| Ok(file_system.read_symlink(path, buffer.as_mut_slice())?)).await {
            Ok(read) => {
                if read != size {
                    return Err(Error::FileSystem(file_system::Error::InvalidSymbolicLink));
                }
                return Ok(read);
            }
            Err(Error::FileSystem(file_system::Error::UnsupportedOperation)) => {}
            Err(error) => return Err(error),
        }

        let mut context = Context::new_empty();
        poll(|| Ok(file_system.lookup_file(&mut context, path, Flags::from(AccessFlags::Read))?))
            .await?;
        let read = file_system.read_sync(&mut context, buffer.as_mut_slice(), 0);
        let close = exported_file_system::BaseOperations::close(file_system, &mut context);
        let read = read?;
        close?;
        if read != size {
            return Err(Error::FileSystem(file_system::Error::InvalidSymbolicLink));
        }
        Ok(read)
    }

    /// Resolve symlinks component by component. Canonical paths without links
    /// stay borrowed; scratch storage is created only after normalization or a link.
    pub(super) async fn resolve_path<'a>(
        &self,
        file_systems: &FileSystemsArray,
        path: &'a Path,
        follow_final: bool,
        allow_missing_final: bool,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<ResolvedPath<'a>> {
        let require_directory = path.as_str().len() > 1 && path.as_str().ends_with('/');
        let resolved = if require_directory {
            let normalized = PathOwned::try_from(path.as_str().trim_end_matches('/'))
                .map_err(|_| Error::InvalidPath)?;
            let resolved = self
                .resolve_path_inner(
                    file_systems,
                    &normalized,
                    follow_final,
                    allow_missing_final,
                    user,
                    task,
                )
                .await?;
            ResolvedPath::Owned(
                PathOwned::try_from(resolved.as_str()).map_err(|_| Error::InvalidPath)?,
            )
        } else {
            self.resolve_path_inner(
                file_systems,
                path,
                follow_final,
                allow_missing_final,
                user,
                task,
            )
            .await?
        };
        if require_directory {
            let (file_system, relative_path, _) =
                Self::get_file_system_from_path(file_systems, &resolved)?;
            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            match Self::get_attributes(file_system.file_system, relative_path, &mut attributes)
                .await
            {
                Ok(()) => {}
                Err(Error::FileSystem(file_system::Error::NotFound)) if allow_missing_final => {
                    return Ok(resolved);
                }
                Err(error) => return Err(error),
            }
            if attributes.get_kind() != Some(&Kind::Directory) {
                return Err(Error::NotADirectory);
            }
        }
        Ok(resolved)
    }

    async fn resolve_path_inner<'a>(
        &self,
        file_systems: &FileSystemsArray,
        path: &'a Path,
        follow_final: bool,
        allow_missing_final: bool,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<ResolvedPath<'a>> {
        const MAX_PATH_LENGTH: usize = 4096;
        if !path.is_valid() || !path.is_absolute() || path.get_length() > MAX_PATH_LENGTH {
            return Err(Error::InvalidPath);
        }

        // Fast path: walk borrowed prefixes first. This avoids allocating a
        // String/PathOwned and a target buffer for ordinary paths.
        let text = path.as_str();
        let mut component_start = 1;
        let mut parent_end = 1;
        let mut target_buffer = Vec::new();
        while component_start < text.len() {
            let component_end = text[component_start..]
                .find('/')
                .map_or(text.len(), |offset| component_start + offset);
            let component = &text[component_start..component_end];
            if component.is_empty() || component == "." || component == ".." {
                break;
            }
            let has_remaining = component_end < text.len();
            let parent = Path::from_str(&text[..parent_end]);
            let (parent_file_system, parent_relative, _) =
                Self::get_file_system_from_path(file_systems, &parent)?;
            self.check_permissions(
                parent_file_system.file_system,
                parent_relative,
                Permission::Execute,
                user,
                task,
            )
            .await?;

            let candidate = Path::from_str(&text[..component_end]);
            let (file_system, relative_path, _) =
                Self::get_file_system_from_path(file_systems, &candidate)?;
            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            match Self::get_attributes(file_system.file_system, relative_path, &mut attributes)
                .await
            {
                Ok(()) => {}
                Err(Error::FileSystem(file_system::Error::NotFound))
                    if !has_remaining && allow_missing_final =>
                {
                    return Ok(ResolvedPath::Borrowed(path));
                }
                Err(error) => return Err(error),
            }
            let kind = *attributes.get_kind().ok_or(Error::MissingAttribute)?;
            if kind == Kind::SymbolicLink && (has_remaining || follow_final) {
                let length = Self::read_link_payload(
                    file_system.file_system,
                    relative_path,
                    &mut target_buffer,
                )
                .await?;
                let target = core::str::from_utf8(&target_buffer[..length])
                    .map_err(|_| Error::FileSystem(file_system::Error::InvalidSymbolicLink))?;
                if !Path::new(target).is_valid() || target.is_empty() {
                    return Err(Error::FileSystem(file_system::Error::InvalidSymbolicLink));
                }

                let mut pending = String::with_capacity(target.len() + text.len() - component_end);
                pending.push_str(target);
                pending.push_str(&text[component_end..]);
                let resolved = if target.starts_with('/') {
                    PathOwned::root()
                } else {
                    PathOwned::try_from(&text[..parent_end]).map_err(|_| Error::InvalidPath)?
                };
                return self
                    .resolve_pending(
                        file_systems,
                        ResolutionState {
                            resolved,
                            pending,
                            links_followed: 1,
                            target_buffer,
                        },
                        follow_final,
                        allow_missing_final,
                        user,
                        task,
                    )
                    .await;
            }
            if has_remaining && kind != Kind::Directory {
                return Err(Error::NotADirectory);
            }
            parent_end = component_end;
            component_start = component_end + 1;
        }

        if component_start >= text.len() {
            return Ok(ResolvedPath::Borrowed(path));
        }

        // Dot components and repeated separators need a normalized working
        // path. This allocation is outside the common canonical-path flow.
        let pending = String::from(text.strip_prefix('/').unwrap_or(text));
        self.resolve_pending(
            file_systems,
            ResolutionState {
                resolved: PathOwned::root(),
                pending,
                links_followed: 0,
                target_buffer,
            },
            follow_final,
            allow_missing_final,
            user,
            task,
        )
        .await
    }

    async fn resolve_pending<'a>(
        &self,
        file_systems: &FileSystemsArray,
        state: ResolutionState,
        follow_final: bool,
        allow_missing_final: bool,
        user: UserIdentifier,
        task: TaskIdentifier,
    ) -> Result<ResolvedPath<'a>> {
        const MAX_PATH_LENGTH: usize = 4096;
        const MAX_SYMLINKS: usize = 40;

        let ResolutionState {
            mut resolved,
            mut pending,
            mut links_followed,
            mut target_buffer,
        } = state;

        while !pending.is_empty() {
            let separator = pending.find('/').unwrap_or(pending.len());
            let component = &pending[..separator];
            let mut consumed = separator;
            while consumed < pending.len() && pending.as_bytes()[consumed] == b'/' {
                consumed += 1;
            }
            let has_remaining = consumed < pending.len();

            if component.is_empty() || component == "." {
                pending.drain(..consumed.max(1));
                continue;
            }

            let (parent_file_system, parent_relative, _) =
                Self::get_file_system_from_path(file_systems, &resolved)?;
            self.check_permissions(
                parent_file_system.file_system,
                parent_relative,
                Permission::Execute,
                user,
                task,
            )
            .await?;

            if component == ".." {
                if let Some(parent) = resolved.go_parent() {
                    resolved = parent.to_owned();
                }
                pending.drain(..consumed.max(1));
                continue;
            }

            let candidate = resolved
                .to_owned()
                .join(Path::from_str(component))
                .ok_or(Error::InvalidPath)?;
            let (file_system, relative_path, _) =
                Self::get_file_system_from_path(file_systems, &candidate)?;
            let mut attributes = Attributes::new().set_mask(AttributeFlags::Kind);
            match Self::get_attributes(file_system.file_system, relative_path, &mut attributes)
                .await
            {
                Ok(()) => {}
                Err(Error::FileSystem(file_system::Error::NotFound))
                    if !has_remaining && allow_missing_final =>
                {
                    return Ok(ResolvedPath::Owned(candidate));
                }
                Err(error) => return Err(error),
            }
            let kind = *attributes.get_kind().ok_or(Error::MissingAttribute)?;

            if kind == Kind::SymbolicLink && (has_remaining || follow_final) {
                links_followed += 1;
                if links_followed > MAX_SYMLINKS {
                    return Err(Error::FileSystem(file_system::Error::InvalidSymbolicLink));
                }
                let target_length = Self::read_link_payload(
                    file_system.file_system,
                    relative_path,
                    &mut target_buffer,
                )
                .await?;
                let target = core::str::from_utf8(&target_buffer[..target_length])
                    .map_err(|_| Error::FileSystem(file_system::Error::InvalidSymbolicLink))?;
                if target.is_empty()
                    || target_length > MAX_PATH_LENGTH
                    || !Path::new(target).is_valid()
                {
                    return Err(Error::FileSystem(file_system::Error::InvalidSymbolicLink));
                }

                if target.starts_with('/') {
                    resolved = PathOwned::root();
                }
                pending.drain(..consumed);
                let target = target.trim_start_matches('/');
                if !target.is_empty() && !pending.is_empty() {
                    pending.insert(0, '/');
                }
                pending.insert_str(0, target);
                if resolved.get_length().saturating_add(pending.len()) > MAX_PATH_LENGTH {
                    return Err(Error::FileSystem(file_system::Error::NameTooLong));
                }
                continue;
            }

            if has_remaining && kind != Kind::Directory {
                return Err(Error::NotADirectory);
            }
            resolved = candidate;
            pending.drain(..consumed);
        }

        Ok(ResolvedPath::Owned(resolved))
    }

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

        // POSIX selects exactly one class; denied owner/group permissions must
        // never fall through to a more permissive class.
        if current_user == owner_user {
            return permissions.get_user().contains(necessary_permissions);
        }
        if users_manager.is_in_group(current_user, owner_group).await {
            return permissions.get_group().contains(necessary_permissions);
        }
        permissions.get_others().contains(necessary_permissions)
    }

    /// Metadata policy shared by namespace and retained handle contexts.
    /// Ownership changes are root-only; chmod permits the owner or root.
    pub(crate) fn authorize_metadata_change(
        current_user: UserIdentifier,
        owner: UserIdentifier,
        changes: &Attributes,
    ) -> Result<()> {
        if current_user == UserIdentifier::ROOT || changes.get_mask().is_empty() {
            return Ok(());
        }
        if changes
            .get_mask()
            .intersects(AttributeFlags::User | AttributeFlags::Group)
            || current_user != owner
        {
            return Err(Error::PermissionDenied);
        }
        Ok(())
    }

    /// Namespace mutations may not invalidate a visible mount or its ancestors.
    /// Call while holding the mount table write guard (before backend mutation).
    pub(super) fn protects_mount(
        file_systems: &FileSystemsArray,
        path: &Path,
        include_self: bool,
    ) -> bool {
        file_systems.iter().any(|fs| {
            fs.is_mounted
                && (include_self || fs.mount_point.as_str() != path.as_str())
                && fs
                    .mount_point
                    .get_components()
                    .strip_prefix(&path.get_components())
                    .is_some()
        })
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
        let time: Time = self.time_manager.realtime_now()?.into();

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
                if !application_access_allowed(path.as_ref(), task_application, asked_permissions) {
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
